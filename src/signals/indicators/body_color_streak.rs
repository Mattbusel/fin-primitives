//! Body Color Streak indicator.
//!
//! Counts how many consecutive bars have had the same body color (bullish =
//! close > open, bearish = close < open). Resets on color change or neutral bar.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Count of consecutive bars with the same body color.
///
/// Colors:
/// - **Bullish** (`close > open`): streak counts while consecutive.
/// - **Bearish** (`close < open`): streak counts while consecutive.
/// - **Neutral** (`close == open`): breaks any streak, resets to 1.
///
/// The returned value is always ≥ 1 after the first bar. The sign encodes
/// direction: positive for bullish streaks, negative for bearish streaks,
/// zero is returned only when the bar is neutral (one-bar neutral).
///
/// Actually the returned value is signed: `+N` for N-bar bullish streak,
/// `-N` for N-bar bearish streak, `0` for a neutral bar.
///
/// `period()` always returns `1`. Ready after the first bar.
///
/// # Errors
/// Never returns an error.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyColorStreak;
/// use fin_primitives::signals::Signal;
///
/// let bcs = BodyColorStreak::new("bcs");
/// assert_eq!(bcs.period(), 1);
/// ```
pub struct BodyColorStreak {
    name: String,
    streak: i32,
    color: i8,    // 1=bull, -1=bear, 0=neutral, 127=none
    seen_bars: usize,
}

impl BodyColorStreak {
    /// Constructs a new `BodyColorStreak`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            streak: 0,
            color: 127,
            seen_bars: 0,
        }
    }
}

impl crate::signals::Signal for BodyColorStreak {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        self.seen_bars >= 1
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.seen_bars += 1;

        let cur_color: i8 = if bar.close > bar.open {
            1
        } else if bar.close < bar.open {
            -1
        } else {
            0
        };

        if self.color == 127 || self.color != cur_color {
            self.streak = if cur_color == 1 { 1 } else if cur_color == -1 { -1 } else { 0 };
        } else {
            // Same color: extend streak
            if cur_color == 1 {
                self.streak += 1;
            } else if cur_color == -1 {
                self.streak -= 1;
            }
        }

        self.color = cur_color;
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.streak = 0;
        self.color = 127;
        self.seen_bars = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, close: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let (high, low) = if c >= o { (c, o) } else { (o, c) };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o, high, low, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bcs_ready_after_first_bar() {
        let mut bcs = BodyColorStreak::new("bcs");
        bcs.update_bar(&bar("100", "105")).unwrap();
        assert!(bcs.is_ready());
    }

    #[test]
    fn test_bcs_bullish_first_bar_returns_one() {
        let mut bcs = BodyColorStreak::new("bcs");
        let v = bcs.update_bar(&bar("100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bcs_bearish_first_bar_returns_neg_one() {
        let mut bcs = BodyColorStreak::new("bcs");
        let v = bcs.update_bar(&bar("105", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_bcs_consecutive_bullish_increments() {
        let mut bcs = BodyColorStreak::new("bcs");
        bcs.update_bar(&bar("100", "105")).unwrap();
        bcs.update_bar(&bar("105", "110")).unwrap();
        let v = bcs.update_bar(&bar("110", "115")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_bcs_consecutive_bearish_decrements() {
        let mut bcs = BodyColorStreak::new("bcs");
        bcs.update_bar(&bar("110", "105")).unwrap();
        bcs.update_bar(&bar("105", "100")).unwrap();
        let v = bcs.update_bar(&bar("100", "95")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-3)));
    }

    #[test]
    fn test_bcs_color_flip_resets() {
        let mut bcs = BodyColorStreak::new("bcs");
        bcs.update_bar(&bar("100", "105")).unwrap();
        bcs.update_bar(&bar("105", "110")).unwrap();
        // Now bearish
        let v = bcs.update_bar(&bar("110", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_bcs_neutral_bar_returns_zero() {
        let mut bcs = BodyColorStreak::new("bcs");
        bcs.update_bar(&bar("100", "105")).unwrap();
        let v = bcs.update_bar(&bar("105", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bcs_reset() {
        let mut bcs = BodyColorStreak::new("bcs");
        bcs.update_bar(&bar("100", "105")).unwrap();
        assert!(bcs.is_ready());
        bcs.reset();
        assert!(!bcs.is_ready());
    }
}
