//! Volatility Bands indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volatility Bands — ATR-based bands around an EMA.
///
/// ```text
/// EMA_t     = EMA(close, period)
/// ATR_t     = mean(TR, period)
/// upper     = EMA_t + multiplier × ATR_t
/// lower     = EMA_t − multiplier × ATR_t
/// output    = (close − EMA_t) / ATR_t   (position in ATR units)
/// ```
///
/// Output near `+multiplier` means close is near the upper band;
/// near `−multiplier` means near the lower band.
/// Zero means close equals the EMA.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityBands;
/// use fin_primitives::signals::Signal;
///
/// let vb = VolatilityBands::new("vb", 14, "2.0".parse().unwrap()).unwrap();
/// assert_eq!(vb.period(), 14);
/// ```
pub struct VolatilityBands {
    name: String,
    period: usize,
    multiplier: Decimal,
    k: Decimal,
    ema: Option<Decimal>,
    seed: Vec<Decimal>,
    trs: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    upper: Option<Decimal>,
    lower: Option<Decimal>,
}

impl VolatilityBands {
    /// Creates a new `VolatilityBands`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if `multiplier` is not positive.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        multiplier: Decimal,
    ) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        if multiplier <= Decimal::ZERO {
            return Err(FinError::InvalidInput("multiplier must be positive".into()));
        }
        let k = Decimal::from(2u32) / Decimal::from((period + 1) as u32);
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            k,
            ema: None,
            seed: Vec::with_capacity(period),
            trs: VecDeque::with_capacity(period),
            prev_close: None,
            upper: None,
            lower: None,
        })
    }

    /// Returns the current upper band.
    pub fn upper(&self) -> Option<Decimal> { self.upper }
    /// Returns the current lower band.
    pub fn lower(&self) -> Option<Decimal> { self.lower }
}

impl Signal for VolatilityBands {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // True range
        let tr = match self.prev_close {
            None => bar.high - bar.low,
            Some(pc) => (bar.high - bar.low)
                .max((bar.high - pc).abs())
                .max((bar.low - pc).abs()),
        };
        self.prev_close = Some(bar.close);

        self.trs.push_back(tr);
        if self.trs.len() > self.period { self.trs.pop_front(); }

        // EMA
        if self.ema.is_none() {
            self.seed.push(bar.close);
            if self.seed.len() == self.period {
                let sma = self.seed.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
                self.ema = Some(sma);
            }
        } else {
            let e = self.ema.unwrap() * (Decimal::ONE - self.k) + bar.close * self.k;
            self.ema = Some(e);
        }

        if self.trs.len() < self.period || self.ema.is_none() {
            return Ok(SignalValue::Unavailable);
        }

        let ema = self.ema.unwrap();
        let atr = self.trs.iter().sum::<Decimal>() / Decimal::from(self.period as u32);

        self.upper = Some(ema + self.multiplier * atr);
        self.lower = Some(ema - self.multiplier * atr);

        if atr.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar((bar.close - ema) / atr))
    }

    fn is_ready(&self) -> bool { self.upper.is_some() }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.ema = None;
        self.seed.clear();
        self.trs.clear();
        self.prev_close = None;
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
    fn test_vb_invalid() {
        assert!(VolatilityBands::new("v", 0, dec!(2)).is_err());
        assert!(VolatilityBands::new("v", 14, dec!(0)).is_err());
        assert!(VolatilityBands::new("v", 14, dec!(-1)).is_err());
    }

    #[test]
    fn test_vb_unavailable_before_warmup() {
        let mut v = VolatilityBands::new("v", 3, dec!(2)).unwrap();
        assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vb_flat_is_zero() {
        // Flat: ATR=0 → returns Scalar(0)
        let mut v = VolatilityBands::new("v", 3, dec!(2)).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = v.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vb_bands_set() {
        let mut v = VolatilityBands::new("v", 3, dec!(2)).unwrap();
        let bars = ["95", "100", "105", "100"];
        for b in &bars { v.update_bar(&bar(b)).unwrap(); }
        assert!(v.upper().is_some());
        assert!(v.lower().is_some());
        assert!(v.upper().unwrap() > v.lower().unwrap());
    }

    #[test]
    fn test_vb_reset() {
        let mut v = VolatilityBands::new("v", 3, dec!(2)).unwrap();
        for _ in 0..5 { v.update_bar(&bar("100")).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
        assert!(v.upper().is_none());
        assert!(v.lower().is_none());
    }
}
