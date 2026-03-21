//! Close Above Open Streak indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close Above Open Streak.
///
/// Counts the current consecutive streak of bars where `close > open` (bullish)
/// or `close < open` (bearish), returning a signed count:
/// - Positive value: current streak of bullish bars.
/// - Negative value: current streak of bearish bars.
/// - Zero: current bar is a doji (close == open), resets streak.
///
/// Does not use a rolling window — the streak can grow indefinitely.
/// Always returns a value from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAboveOpenStreak;
/// use fin_primitives::signals::Signal;
/// let s = CloseAboveOpenStreak::new("caos");
/// assert_eq!(s.period(), 1);
/// ```
pub struct CloseAboveOpenStreak {
    name: String,
    streak: i64,
}

impl CloseAboveOpenStreak {
    /// Constructs a new `CloseAboveOpenStreak`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), streak: 0 }
    }
}

impl Signal for CloseAboveOpenStreak {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.close > bar.open {
            // Bullish: extend bullish streak or start new
            self.streak = if self.streak > 0 { self.streak + 1 } else { 1 };
        } else if bar.close < bar.open {
            // Bearish: extend bearish streak or start new
            self.streak = if self.streak < 0 { self.streak - 1 } else { -1 };
        } else {
            // Doji: reset
            self.streak = 0;
        }
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn period(&self) -> usize {
        1
    }

    fn reset(&mut self) {
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

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let hi = if op > cl { op } else { cl };
        let lo = if op < cl { op } else { cl };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bullish_streak() {
        let mut s = CloseAboveOpenStreak::new("caos");
        assert_eq!(s.update_bar(&bar("10", "12")).unwrap(), SignalValue::Scalar(dec!(1)));
        assert_eq!(s.update_bar(&bar("12", "14")).unwrap(), SignalValue::Scalar(dec!(2)));
        assert_eq!(s.update_bar(&bar("14", "16")).unwrap(), SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_bearish_streak() {
        let mut s = CloseAboveOpenStreak::new("caos");
        assert_eq!(s.update_bar(&bar("12", "10")).unwrap(), SignalValue::Scalar(dec!(-1)));
        assert_eq!(s.update_bar(&bar("10", "8")).unwrap(), SignalValue::Scalar(dec!(-2)));
    }

    #[test]
    fn test_doji_resets_streak() {
        let mut s = CloseAboveOpenStreak::new("caos");
        s.update_bar(&bar("10", "12")).unwrap();
        s.update_bar(&bar("12", "14")).unwrap();
        assert_eq!(s.update_bar(&bar("14", "14")).unwrap(), SignalValue::Scalar(dec!(0)));
        // New streak after doji
        assert_eq!(s.update_bar(&bar("10", "12")).unwrap(), SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_direction_switch_resets_to_one() {
        let mut s = CloseAboveOpenStreak::new("caos");
        s.update_bar(&bar("10", "12")).unwrap();
        s.update_bar(&bar("12", "14")).unwrap();
        // Switch to bearish
        assert_eq!(s.update_bar(&bar("14", "10")).unwrap(), SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_reset() {
        let mut s = CloseAboveOpenStreak::new("caos");
        s.update_bar(&bar("10", "12")).unwrap();
        s.update_bar(&bar("12", "14")).unwrap();
        s.reset();
        assert_eq!(s.update_bar(&bar("10", "12")).unwrap(), SignalValue::Scalar(dec!(1)));
    }
}
