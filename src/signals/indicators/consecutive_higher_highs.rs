//! Consecutive Higher Highs indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Consecutive Higher Highs.
///
/// Counts the current consecutive streak of bars making higher highs than the previous bar.
/// Returns a signed integer:
/// - Positive: consecutive higher highs count.
/// - Negative: consecutive lower highs count.
/// - 0: equal high to previous bar (streak reset).
///
/// Useful for identifying trend strength and potential breakout conditions.
/// Always returns a value from the second bar onward.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ConsecutiveHigherHighs;
/// use fin_primitives::signals::Signal;
/// let chh = ConsecutiveHigherHighs::new("chh");
/// assert_eq!(chh.period(), 2);
/// ```
pub struct ConsecutiveHigherHighs {
    name: String,
    prev_high: Option<Decimal>,
    streak: i64,
}

impl ConsecutiveHigherHighs {
    /// Constructs a new `ConsecutiveHigherHighs`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_high: None, streak: 0 }
    }
}

impl Signal for ConsecutiveHigherHighs {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(prev) = self.prev_high {
            if bar.high > prev {
                self.streak = if self.streak > 0 { self.streak + 1 } else { 1 };
            } else if bar.high < prev {
                self.streak = if self.streak < 0 { self.streak - 1 } else { -1 };
            } else {
                self.streak = 0;
            }
            SignalValue::Scalar(Decimal::from(self.streak))
        } else {
            SignalValue::Unavailable
        };

        self.prev_high = Some(bar.high);
        Ok(result)
    }

    fn is_ready(&self) -> bool {
        self.prev_high.is_some()
    }

    fn period(&self) -> usize {
        2
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.streak = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(high: &str) -> OhlcvBar {
        let h = Price::new(high.parse().unwrap()).unwrap();
        let l = Price::new("1".parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l, high: h, low: l, close: h,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_first_bar_unavailable() {
        let mut chh = ConsecutiveHigherHighs::new("chh");
        assert_eq!(chh.update_bar(&bar("15")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_consecutive_higher_highs() {
        let mut chh = ConsecutiveHigherHighs::new("chh");
        chh.update_bar(&bar("10")).unwrap();
        assert_eq!(chh.update_bar(&bar("12")).unwrap(), SignalValue::Scalar(dec!(1)));
        assert_eq!(chh.update_bar(&bar("14")).unwrap(), SignalValue::Scalar(dec!(2)));
        assert_eq!(chh.update_bar(&bar("16")).unwrap(), SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_consecutive_lower_highs() {
        let mut chh = ConsecutiveHigherHighs::new("chh");
        chh.update_bar(&bar("16")).unwrap();
        assert_eq!(chh.update_bar(&bar("14")).unwrap(), SignalValue::Scalar(dec!(-1)));
        assert_eq!(chh.update_bar(&bar("12")).unwrap(), SignalValue::Scalar(dec!(-2)));
    }

    #[test]
    fn test_equal_resets() {
        let mut chh = ConsecutiveHigherHighs::new("chh");
        chh.update_bar(&bar("10")).unwrap();
        chh.update_bar(&bar("12")).unwrap();
        chh.update_bar(&bar("14")).unwrap();
        assert_eq!(chh.update_bar(&bar("14")).unwrap(), SignalValue::Scalar(dec!(0)));
        assert_eq!(chh.update_bar(&bar("15")).unwrap(), SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_reset() {
        let mut chh = ConsecutiveHigherHighs::new("chh");
        chh.update_bar(&bar("10")).unwrap();
        chh.update_bar(&bar("12")).unwrap();
        assert!(chh.is_ready());
        chh.reset();
        assert!(!chh.is_ready());
    }
}
