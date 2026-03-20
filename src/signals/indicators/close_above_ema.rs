//! Close-Above-EMA ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close-Above-EMA Ratio -- percentage of the last `window` bars where close > EMA(period).
///
/// Measures bullish EMA position consistency. High values (>70) indicate price has been
/// persistently above its moving average (uptrend). Low values (<30) indicate a downtrend.
///
/// # Parameters
/// - `ema_period`: EMA smoothing period
/// - `window`: rolling look-back window for the fraction calculation
///
/// Returns [`SignalValue::Unavailable`] until the EMA has warmed up (`ema_period` bars)
/// and the window is full (`window` bars).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAboveEma;
/// use fin_primitives::signals::Signal;
/// let cae = CloseAboveEma::new("cae", 20, 10).unwrap();
/// assert_eq!(cae.period(), 20);
/// ```
pub struct CloseAboveEma {
    name: String,
    ema_period: usize,
    window_size: usize,
    k: Decimal,
    ema: Option<Decimal>,
    ema_bars: usize,
    results: VecDeque<u8>,
    count: usize,
}

impl CloseAboveEma {
    /// Constructs a new `CloseAboveEma`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is 0.
    pub fn new(name: impl Into<String>, ema_period: usize, window: usize) -> Result<Self, FinError> {
        if ema_period == 0 { return Err(FinError::InvalidPeriod(ema_period)); }
        if window == 0 { return Err(FinError::InvalidPeriod(window)); }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::TWO / (Decimal::from(ema_period as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            ema_period,
            window_size: window,
            k,
            ema: None,
            ema_bars: 0,
            results: VecDeque::with_capacity(window),
            count: 0,
        })
    }

    /// Returns the EMA period.
    pub fn ema_period(&self) -> usize { self.ema_period }
}

impl Signal for CloseAboveEma {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.ema_period }
    fn is_ready(&self) -> bool { self.results.len() >= self.window_size }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        self.ema = Some(match self.ema {
            None => close,
            Some(prev) => self.k * close + (Decimal::ONE - self.k) * prev,
        });
        self.ema_bars += 1;

        if self.ema_bars <= self.ema_period { return Ok(SignalValue::Unavailable); }

        let above: u8 = if close > self.ema.unwrap_or(Decimal::ZERO) { 1 } else { 0 };
        self.results.push_back(above);
        self.count += above as usize;
        if self.results.len() > self.window_size {
            if let Some(old) = self.results.pop_front() { self.count -= old as usize; }
        }
        if self.results.len() < self.window_size { return Ok(SignalValue::Unavailable); }

        #[allow(clippy::cast_possible_truncation)]
        let ratio = Decimal::from(self.count as u32)
            / Decimal::from(self.window_size as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.ema = None;
        self.ema_bars = 0;
        self.results.clear();
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_cae_period_0_error() { assert!(CloseAboveEma::new("c", 0, 5).is_err()); }
    #[test]
    fn test_cae_window_0_error() { assert!(CloseAboveEma::new("c", 10, 0).is_err()); }

    #[test]
    fn test_cae_unavailable_before_warmup() {
        let mut c = CloseAboveEma::new("c", 3, 2).unwrap();
        for _ in 0..3 {
            assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_cae_all_above_is_100() {
        // constant price, EMA == price, close NOT > ema (equal), so 0%
        // use rising prices to be above
        let mut c = CloseAboveEma::new("c", 3, 3).unwrap();
        // warm up EMA
        for _ in 0..3 { c.update_bar(&bar("100")).unwrap(); }
        // now push bars well above 100 (EMA will be ~ 100)
        for _ in 0..3 { c.update_bar(&bar("200")).unwrap(); }
        if let SignalValue::Scalar(v) = c.update_bar(&bar("200")).unwrap() {
            assert_eq!(v, dec!(100));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cae_reset() {
        let mut c = CloseAboveEma::new("c", 3, 2).unwrap();
        for _ in 0..10 { c.update_bar(&bar("100")).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
