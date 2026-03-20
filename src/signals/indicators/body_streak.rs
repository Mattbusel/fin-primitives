//! Body Streak indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Body Streak — counts consecutive candles with the same body direction.
///
/// A candle is **bullish** when `close > open`, **bearish** when `close < open`,
/// and neutral (doji) otherwise.
///
/// Outputs:
/// - `+n` → `n` consecutive bullish candles
/// - `-n` → `n` consecutive bearish candles
/// - `0` → current bar is a doji (resets streak)
///
/// Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyStreak;
/// use fin_primitives::signals::Signal;
///
/// let bs = BodyStreak::new("bs").unwrap();
/// assert_eq!(bs.period(), 1);
/// ```
pub struct BodyStreak {
    name: String,
    streak: i64,
}

impl BodyStreak {
    /// Constructs a new `BodyStreak`.
    ///
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), streak: 0 })
    }

    /// Returns the current streak count.
    pub fn streak(&self) -> i64 {
        self.streak
    }
}

impl Signal for BodyStreak {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        let open = bar.open;

        self.streak = if close > open {
            if self.streak >= 0 { self.streak + 1 } else { 1 }
        } else if close < open {
            if self.streak <= 0 { self.streak - 1 } else { -1 }
        } else {
            0
        };

        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
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
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let hp = Price::new(cp.value().max(op.value()).to_string().parse().unwrap()).unwrap();
        let lp = Price::new(cp.value().min(op.value()).to_string().parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_body_streak_always_ready() {
        let bs = BodyStreak::new("bs").unwrap();
        assert!(bs.is_ready());
    }

    #[test]
    fn test_body_streak_bullish_accumulates() {
        let mut bs = BodyStreak::new("bs").unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..4 {
            last = bs.update_bar(&bar("100", "105")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(4)));
    }

    #[test]
    fn test_body_streak_bearish_accumulates() {
        let mut bs = BodyStreak::new("bs").unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = bs.update_bar(&bar("105", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(-3)));
    }

    #[test]
    fn test_body_streak_doji_resets() {
        let mut bs = BodyStreak::new("bs").unwrap();
        for _ in 0..3 { bs.update_bar(&bar("100", "105")).unwrap(); }
        let result = bs.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_body_streak_direction_change_resets_to_one() {
        let mut bs = BodyStreak::new("bs").unwrap();
        for _ in 0..3 { bs.update_bar(&bar("100", "105")).unwrap(); }
        let result = bs.update_bar(&bar("105", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_body_streak_reset() {
        let mut bs = BodyStreak::new("bs").unwrap();
        for _ in 0..3 { bs.update_bar(&bar("100", "105")).unwrap(); }
        bs.reset();
        assert_eq!(bs.streak(), 0);
    }
}
