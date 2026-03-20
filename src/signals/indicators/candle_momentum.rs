//! Candle Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Candle Momentum — rolling sum of signed candle body sizes.
///
/// ```text
/// body_t = close_t − open_t     (positive = bull bar, negative = bear bar)
/// output = mean(body, period)
/// ```
///
/// Positive output indicates dominant bullish candle bodies; negative bearish.
/// Normalises by dividing by the period so it's comparable across periods.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CandleMomentum;
/// use fin_primitives::signals::Signal;
///
/// let cm = CandleMomentum::new("cm", 10).unwrap();
/// assert_eq!(cm.period(), 10);
/// ```
pub struct CandleMomentum {
    name: String,
    period: usize,
    bodies: VecDeque<Decimal>,
}

impl CandleMomentum {
    /// Creates a new `CandleMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            bodies: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CandleMomentum {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.close - bar.open;
        self.bodies.push_back(body);
        if self.bodies.len() > self.period { self.bodies.pop_front(); }
        if self.bodies.len() < self.period { return Ok(SignalValue::Unavailable); }

        let avg = self.bodies.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool { self.bodies.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.bodies.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_oc(o: &str, c: &str) -> OhlcvBar {
        let ov: rust_decimal::Decimal = o.parse().unwrap();
        let cv: rust_decimal::Decimal = c.parse().unwrap();
        let op = Price::new(ov).unwrap();
        let cp = Price::new(cv).unwrap();
        let hp = Price::new(ov.max(cv)).unwrap();
        let lp = Price::new(ov.min(cv)).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn doji() -> OhlcvBar { bar_oc("100", "100") }

    #[test]
    fn test_cm_invalid() {
        assert!(CandleMomentum::new("c", 0).is_err());
    }

    #[test]
    fn test_cm_unavailable_before_warmup() {
        let mut c = CandleMomentum::new("c", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(c.update_bar(&doji()).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_cm_doji_is_zero() {
        let mut c = CandleMomentum::new("c", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = c.update_bar(&doji()).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_cm_all_bull_positive() {
        // body = close-open = 5 each bar → mean = 5
        let mut c = CandleMomentum::new("c", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = c.update_bar(&bar_oc("100", "105")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(5));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_cm_all_bear_negative() {
        let mut c = CandleMomentum::new("c", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = c.update_bar(&bar_oc("105", "100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(-5));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_cm_alternating_zero_mean() {
        // +5, -5, +5, -5 → mean = 0
        let mut c = CandleMomentum::new("c", 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            c.update_bar(&bar_oc("100", "105")).unwrap();
            last = c.update_bar(&bar_oc("105", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_cm_reset() {
        let mut c = CandleMomentum::new("c", 3).unwrap();
        for _ in 0..5 { c.update_bar(&doji()).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
