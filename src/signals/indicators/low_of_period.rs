//! Low of Period indicator -- rolling N-bar lowest low.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Low of Period -- the lowest low seen over the last `period` bars.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LowOfPeriod;
/// use fin_primitives::signals::Signal;
/// let l = LowOfPeriod::new("lop", 20).unwrap();
/// assert_eq!(l.period(), 20);
/// ```
pub struct LowOfPeriod {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl LowOfPeriod {
    /// Constructs a new `LowOfPeriod`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self { name: name.into(), period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for LowOfPeriod {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.low);
        if self.window.len() > self.period { self.window.pop_front(); }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        let min = self.window.iter().copied().fold(Decimal::MAX, Decimal::min);
        Ok(SignalValue::Scalar(min))
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(l: &str) -> OhlcvBar {
        let p = Price::new(l.parse().unwrap()).unwrap();
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
    fn test_lop_period_0_error() { assert!(LowOfPeriod::new("l", 0).is_err()); }

    #[test]
    fn test_lop_unavailable_before_period() {
        let mut l = LowOfPeriod::new("l", 3).unwrap();
        assert_eq!(l.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_lop_returns_min() {
        let mut l = LowOfPeriod::new("l", 3).unwrap();
        l.update_bar(&bar("90")).unwrap();
        l.update_bar(&bar("110")).unwrap();
        let v = l.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(90)));
    }

    #[test]
    fn test_lop_rolls_out_old_min() {
        let mut l = LowOfPeriod::new("l", 3).unwrap();
        l.update_bar(&bar("50")).unwrap(); // will roll out
        l.update_bar(&bar("90")).unwrap();
        l.update_bar(&bar("95")).unwrap(); // window full, min=50
        let v = l.update_bar(&bar("80")).unwrap(); // 50 rolls out, min=80
        assert_eq!(v, SignalValue::Scalar(dec!(80)));
    }

    #[test]
    fn test_lop_reset() {
        let mut l = LowOfPeriod::new("l", 2).unwrap();
        l.update_bar(&bar("100")).unwrap();
        l.update_bar(&bar("90")).unwrap();
        assert!(l.is_ready());
        l.reset();
        assert!(!l.is_ready());
    }
}
