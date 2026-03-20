//! Consecutive Bars indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Consecutive Bars — counts consecutive up or down bars.
///
/// ```text
/// count_t = count_{t-1} + 1  if close_t > close_{t-1} (up bar)
/// count_t = count_{t-1} - 1  if close_t < close_{t-1} (down bar)
/// count_t = 0                if close_t == close_{t-1}
/// ```
///
/// Direction resets to ±1 on reversal. Positive values indicate consecutive up
/// bars; negative indicate consecutive down bars.
///
/// Returns [`SignalValue::Unavailable`] on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ConsecutiveBars;
/// use fin_primitives::signals::Signal;
///
/// let cb = ConsecutiveBars::new("cb").unwrap();
/// assert_eq!(cb.period(), 1);
/// ```
pub struct ConsecutiveBars {
    name: String,
    prev_close: Option<Decimal>,
    count: i32,
}

impl ConsecutiveBars {
    /// Creates a new `ConsecutiveBars`.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_close: None, count: 0 })
    }

    /// Returns the current consecutive bar count.
    pub fn count(&self) -> i32 { self.count }
}

impl Signal for ConsecutiveBars {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let prev = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(p) => p,
        };
        self.prev_close = Some(bar.close);

        self.count = if bar.close > prev {
            if self.count > 0 { self.count + 1 } else { 1 }
        } else if bar.close < prev {
            if self.count < 0 { self.count - 1 } else { -1 }
        } else {
            0
        };

        Ok(SignalValue::Scalar(Decimal::from(self.count)))
    }

    fn is_ready(&self) -> bool { self.prev_close.is_some() }
    fn period(&self) -> usize { 1 }

    fn reset(&mut self) {
        self.prev_close = None;
        self.count = 0;
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
    fn test_cb_unavailable_first_bar() {
        let mut cb = ConsecutiveBars::new("cb").unwrap();
        assert_eq!(cb.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cb_consecutive_up() {
        let mut cb = ConsecutiveBars::new("cb").unwrap();
        cb.update_bar(&bar("100")).unwrap();
        assert_eq!(cb.update_bar(&bar("101")).unwrap(), SignalValue::Scalar(dec!(1)));
        assert_eq!(cb.update_bar(&bar("102")).unwrap(), SignalValue::Scalar(dec!(2)));
        assert_eq!(cb.update_bar(&bar("103")).unwrap(), SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_cb_consecutive_down() {
        let mut cb = ConsecutiveBars::new("cb").unwrap();
        cb.update_bar(&bar("100")).unwrap();
        assert_eq!(cb.update_bar(&bar("99")).unwrap(), SignalValue::Scalar(dec!(-1)));
        assert_eq!(cb.update_bar(&bar("98")).unwrap(), SignalValue::Scalar(dec!(-2)));
        assert_eq!(cb.update_bar(&bar("97")).unwrap(), SignalValue::Scalar(dec!(-3)));
    }

    #[test]
    fn test_cb_reversal_resets() {
        let mut cb = ConsecutiveBars::new("cb").unwrap();
        cb.update_bar(&bar("100")).unwrap();
        cb.update_bar(&bar("101")).unwrap();
        cb.update_bar(&bar("102")).unwrap();
        // Reversal: drop
        assert_eq!(cb.update_bar(&bar("101")).unwrap(), SignalValue::Scalar(dec!(-1)));
        // Reversal again: rise
        assert_eq!(cb.update_bar(&bar("102")).unwrap(), SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cb_flat_is_zero() {
        let mut cb = ConsecutiveBars::new("cb").unwrap();
        cb.update_bar(&bar("100")).unwrap();
        assert_eq!(cb.update_bar(&bar("100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cb_reset() {
        let mut cb = ConsecutiveBars::new("cb").unwrap();
        for c in ["100", "101", "102", "103"] { cb.update_bar(&bar(c)).unwrap(); }
        assert!(cb.is_ready());
        assert_eq!(cb.count(), 3);
        cb.reset();
        assert!(!cb.is_ready());
        assert_eq!(cb.count(), 0);
    }
}
