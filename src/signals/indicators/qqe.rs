//! Quantitative Qualitative Estimation (QQE) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// QQE — Quantitative Qualitative Estimation.
///
/// Smooths the RSI with a double-EMA and then computes a dynamic trailing stop
/// band (QQE band) from the ATR of the smoothed RSI.
///
/// ```text
/// RSI_t         = RSI(close, rsi_period)
/// SMRSI_t       = EMA(RSI_t, smooth)
/// DSMRSI_t      = EMA(SMRSI_t, smooth)       // double-smoothed
/// rsi_atr_t     = |DSMRSI_t - DSMRSI_{t-1}|
/// qqe_band_t    = EMA(rsi_atr, atr_period) × factor
/// upper         = DSMRSI + qqe_band
/// lower         = DSMRSI - qqe_band
/// ```
///
/// Returns the double-smoothed RSI as the scalar value.
/// Returns [`SignalValue::Unavailable`] until fully warmed up.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Qqe;
/// use fin_primitives::signals::Signal;
///
/// let q = Qqe::new("qqe", 14, 5, 4, "4.236".parse().unwrap()).unwrap();
/// assert_eq!(q.period(), 14);
/// ```
pub struct Qqe {
    name: String,
    rsi_period: usize,
    smooth: usize,
    atr_period: usize,
    factor: Decimal,
    // RSI state
    gains: VecDeque<Decimal>,
    losses: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    // EMA smoothing state
    smooth_k: Decimal,
    ema1: Option<Decimal>,
    ema2: Option<Decimal>,
    prev_dsmrsi: Option<Decimal>,
    // ATR of DSMRSI
    atr_k: Decimal,
    rsi_atr: Option<Decimal>,
    // Accessors
    upper: Option<Decimal>,
    lower: Option<Decimal>,
}

impl Qqe {
    /// Creates a new `Qqe`.
    ///
    /// - `rsi_period`: RSI lookback (typically 14).
    /// - `smooth`: EMA smoothing period (typically 5).
    /// - `atr_period`: period for ATR of the smoothed RSI (typically 4).
    /// - `factor`: multiplier for band width (typically 4.236).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is zero.
    pub fn new(
        name: impl Into<String>,
        rsi_period: usize,
        smooth: usize,
        atr_period: usize,
        factor: Decimal,
    ) -> Result<Self, FinError> {
        if rsi_period == 0 { return Err(FinError::InvalidPeriod(rsi_period)); }
        if smooth == 0     { return Err(FinError::InvalidPeriod(smooth)); }
        if atr_period == 0 { return Err(FinError::InvalidPeriod(atr_period)); }
        #[allow(clippy::cast_possible_truncation)]
        let smooth_k = Decimal::TWO / Decimal::from((smooth + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let atr_k = Decimal::TWO / Decimal::from((atr_period + 1) as u32);
        Ok(Self {
            name: name.into(),
            rsi_period,
            smooth,
            atr_period,
            factor,
            gains: VecDeque::with_capacity(rsi_period),
            losses: VecDeque::with_capacity(rsi_period),
            prev_close: None,
            smooth_k,
            ema1: None,
            ema2: None,
            prev_dsmrsi: None,
            atr_k,
            rsi_atr: None,
            upper: None,
            lower: None,
        })
    }

    /// Returns the EMA smoothing period.
    pub fn smooth(&self) -> usize { self.smooth }
    /// Returns the ATR lookback period for the band.
    pub fn atr_period(&self) -> usize { self.atr_period }
    /// Returns the current upper QQE band.
    pub fn upper(&self) -> Option<Decimal> { self.upper }
    /// Returns the current lower QQE band.
    pub fn lower(&self) -> Option<Decimal> { self.lower }

    fn compute_rsi(&self) -> Option<Decimal> {
        if self.gains.len() < self.rsi_period { return None; }
        let n = Decimal::from(self.rsi_period as u32);
        let avg_gain = self.gains.iter().sum::<Decimal>() / n;
        let avg_loss = self.losses.iter().sum::<Decimal>() / n;
        if avg_loss.is_zero() { return Some(Decimal::from(100u32)); }
        let rs = avg_gain / avg_loss;
        Some(Decimal::from(100u32) - Decimal::from(100u32) / (Decimal::ONE + rs))
    }

    fn ema_next(prev: &mut Option<Decimal>, val: Decimal, k: Decimal) -> Decimal {
        match *prev {
            None => { *prev = Some(val); val }
            Some(p) => { let v = val * k + p * (Decimal::ONE - k); *prev = Some(v); v }
        }
    }
}

impl Signal for Qqe {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        let prev = match self.prev_close {
            None => { self.prev_close = Some(close); return Ok(SignalValue::Unavailable); }
            Some(p) => p,
        };
        self.prev_close = Some(close);

        let change = close - prev;
        let gain = if change > Decimal::ZERO { change } else { Decimal::ZERO };
        let loss = if change < Decimal::ZERO { -change } else { Decimal::ZERO };
        self.gains.push_back(gain);
        self.losses.push_back(loss);
        if self.gains.len() > self.rsi_period { self.gains.pop_front(); self.losses.pop_front(); }

        let rsi = match self.compute_rsi() {
            None => return Ok(SignalValue::Unavailable),
            Some(r) => r,
        };

        let ema1 = Self::ema_next(&mut self.ema1, rsi, self.smooth_k);
        let dsmrsi = Self::ema_next(&mut self.ema2, ema1, self.smooth_k);

        let rsi_atr_val = match self.prev_dsmrsi {
            None => {
                self.prev_dsmrsi = Some(dsmrsi);
                return Ok(SignalValue::Unavailable);
            }
            Some(prev_ds) => (dsmrsi - prev_ds).abs(),
        };
        self.prev_dsmrsi = Some(dsmrsi);

        let atr = Self::ema_next(&mut self.rsi_atr, rsi_atr_val, self.atr_k);
        let band = atr * self.factor;

        self.upper = Some(dsmrsi + band);
        self.lower = Some(dsmrsi - band);

        Ok(SignalValue::Scalar(dsmrsi))
    }

    fn is_ready(&self) -> bool {
        self.upper.is_some()
    }

    fn period(&self) -> usize {
        self.rsi_period
    }

    fn reset(&mut self) {
        self.gains.clear();
        self.losses.clear();
        self.prev_close = None;
        self.ema1 = None;
        self.ema2 = None;
        self.prev_dsmrsi = None;
        self.rsi_atr = None;
        self.upper = None;
        self.lower = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_qqe_invalid() {
        assert!(Qqe::new("q", 0, 5, 4, dec!(4.236)).is_err());
        assert!(Qqe::new("q", 14, 0, 4, dec!(4.236)).is_err());
        assert!(Qqe::new("q", 14, 5, 0, dec!(4.236)).is_err());
    }

    #[test]
    fn test_qqe_unavailable_before_warmup() {
        let mut q = Qqe::new("q", 3, 2, 2, dec!(4.236)).unwrap();
        for _ in 0..4 {
            assert_eq!(q.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_qqe_produces_scalar() {
        let mut q = Qqe::new("q", 3, 2, 2, dec!(4.236)).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..30usize {
            last = q.update_bar(&bar(&(100 + i % 5).to_string())).unwrap();
        }
        assert!(matches!(last, SignalValue::Scalar(_)), "expected Scalar: {last:?}");
    }

    #[test]
    fn test_qqe_bands_set_when_ready() {
        let mut q = Qqe::new("q", 3, 2, 2, dec!(4.236)).unwrap();
        for i in 0..30usize {
            q.update_bar(&bar(&(100 + i % 5).to_string())).unwrap();
        }
        assert!(q.upper().is_some());
        assert!(q.lower().is_some());
    }

    #[test]
    fn test_qqe_reset() {
        let mut q = Qqe::new("q", 3, 2, 2, dec!(4.236)).unwrap();
        for i in 0..30usize { q.update_bar(&bar(&(100 + i % 5).to_string())).unwrap(); }
        assert!(q.is_ready());
        q.reset();
        assert!(!q.is_ready());
        assert!(q.upper().is_none());
    }
}
