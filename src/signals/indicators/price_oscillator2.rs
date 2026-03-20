//! Price Oscillator 2 indicator -- fast EMA minus slow SMA, percentage-based.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Oscillator 2 -- the percentage difference between a fast EMA and a slow SMA.
///
/// Combines the smoothing of EMA with the simplicity of SMA for a trend/momentum signal.
///
/// ```text
/// fast_ema[t] = EMA(close, fast_period)
/// slow_sma[t] = SMA(close, slow_period)
/// osc[t]      = (fast_ema - slow_sma) / slow_sma * 100
/// ```
///
/// Positive values indicate the EMA is above the SMA (bullish);
/// negative values indicate the EMA is below the SMA (bearish).
///
/// Returns [`SignalValue::Unavailable`] until `slow_period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceOscillator2;
/// use fin_primitives::signals::Signal;
/// let po = PriceOscillator2::new("po", 12, 26).unwrap();
/// assert_eq!(po.period(), 26);
/// ```
pub struct PriceOscillator2 {
    name: String,
    slow_period: usize,
    ema: Option<Decimal>,
    ema_k: Decimal,
    sma_window: VecDeque<Decimal>,
    sma_sum: Decimal,
}

impl PriceOscillator2 {
    /// Constructs a new `PriceOscillator2`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `fast_period == 0`, `slow_period == 0`,
    /// or `fast_period >= slow_period`.
    pub fn new(name: impl Into<String>, fast_period: usize, slow_period: usize) -> Result<Self, FinError> {
        if fast_period == 0 { return Err(FinError::InvalidPeriod(fast_period)); }
        if slow_period == 0 { return Err(FinError::InvalidPeriod(slow_period)); }
        if fast_period >= slow_period { return Err(FinError::InvalidPeriod(fast_period)); }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::TWO / Decimal::from((fast_period + 1) as u32);
        Ok(Self {
            name: name.into(),
            slow_period,
            ema: None,
            ema_k: k,
            sma_window: VecDeque::with_capacity(slow_period),
            sma_sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceOscillator2 {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.slow_period }
    fn is_ready(&self) -> bool { self.sma_window.len() >= self.slow_period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Update EMA
        self.ema = Some(match self.ema {
            None => bar.close,
            Some(prev) => self.ema_k * bar.close + (Decimal::ONE - self.ema_k) * prev,
        });
        // Update SMA
        self.sma_window.push_back(bar.close);
        self.sma_sum += bar.close;
        if self.sma_window.len() > self.slow_period {
            if let Some(old) = self.sma_window.pop_front() { self.sma_sum -= old; }
        }
        if self.sma_window.len() < self.slow_period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let slow_sma = self.sma_sum / Decimal::from(self.slow_period as u32);
        if slow_sma.is_zero() { return Ok(SignalValue::Unavailable); }
        let fast_ema = self.ema.unwrap_or(bar.close);
        Ok(SignalValue::Scalar((fast_ema - slow_sma) / slow_sma * Decimal::ONE_HUNDRED))
    }

    fn reset(&mut self) {
        self.ema = None;
        self.sma_window.clear();
        self.sma_sum = Decimal::ZERO;
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
    fn test_po2_invalid_periods() {
        assert!(PriceOscillator2::new("po", 0, 5).is_err());
        assert!(PriceOscillator2::new("po", 5, 0).is_err());
        assert!(PriceOscillator2::new("po", 5, 5).is_err()); // fast >= slow
        assert!(PriceOscillator2::new("po", 10, 5).is_err()); // fast > slow
    }

    #[test]
    fn test_po2_unavailable_before_slow_period() {
        let mut po = PriceOscillator2::new("po", 2, 5).unwrap();
        assert_eq!(po.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_po2_flat_price_is_zero() {
        // When all prices are the same, EMA == SMA -> oscillator = 0
        let mut po = PriceOscillator2::new("po", 2, 5).unwrap();
        for _ in 0..5 { po.update_bar(&bar("100")).unwrap(); }
        let v = po.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s.abs() < dec!(0.001), "flat prices, oscillator near 0, got {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_po2_rising_prices_positive() {
        // With rising prices, fast EMA rises faster than slow SMA -> positive oscillator
        let mut po = PriceOscillator2::new("po", 2, 4).unwrap();
        for i in 0u32..4 { po.update_bar(&bar(&(100 + i * 5).to_string())).unwrap(); }
        // After warmup: fast EMA will have responded more to recent price rises
        let v = po.update_bar(&bar("125")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0), "rising prices, fast EMA > slow SMA, got {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_po2_reset() {
        let mut po = PriceOscillator2::new("po", 2, 4).unwrap();
        for _ in 0..5 { po.update_bar(&bar("100")).unwrap(); }
        assert!(po.is_ready());
        po.reset();
        assert!(!po.is_ready());
    }
}
