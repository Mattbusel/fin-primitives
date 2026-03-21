//! Bar Polarity Streak indicator.
//!
//! Counts how many consecutive bars have had the same directional polarity
//! (close > open = bullish; close < open = bearish; close == open = neutral).
//! The count increments for each matching bar and resets when polarity changes.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Count of consecutive bars sharing the same polarity relative to `close - open`.
///
/// Polarity categories:
/// - **Bullish** (`close > open`): streak increments with each bullish bar.
/// - **Bearish** (`close < open`): streak increments with each bearish bar.
/// - **Neutral** (`close == open`): treated as a separate polarity; breaks a bullish or bearish streak.
///
/// The value returned is the length of the current streak (always ≥ 1 after the first bar).
/// When polarity flips, the count resets to `1` for the new polarity.
///
/// This is a stateful single-bar indicator; `period()` always returns `1`.
/// Returns a value after the first bar.
///
/// # Errors
/// Never returns an error (signature requires `Result` for trait compatibility).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarPolarityStreak;
/// use fin_primitives::signals::Signal;
///
/// let bps = BarPolarityStreak::new("bps");
/// assert_eq!(bps.period(), 1);
/// ```
pub struct BarPolarityStreak {
    name: String,
    streak: u32,
    polarity: i8, // 1 = bull, -1 = bear, 0 = neutral, None encoded as 127
    seen_bars: usize,
}

impl BarPolarityStreak {
    /// Constructs a new `BarPolarityStreak`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            streak: 0,
            polarity: 127, // sentinel for "no prior bar"
            seen_bars: 0,
        }
    }
}

impl crate::signals::Signal for BarPolarityStreak {
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

        let cur_polarity: i8 = if bar.close > bar.open {
            1
        } else if bar.close < bar.open {
            -1
        } else {
            0
        };

        if self.polarity == 127 || self.polarity != cur_polarity {
            self.streak = 1;
        } else {
            self.streak += 1;
        }

        self.polarity = cur_polarity;
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.streak = 0;
        self.polarity = 127;
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
    fn test_bps_ready_after_first_bar() {
        let mut bps = BarPolarityStreak::new("bps");
        bps.update_bar(&bar("100", "105")).unwrap();
        assert!(bps.is_ready());
    }

    #[test]
    fn test_bps_first_bar_returns_one() {
        let mut bps = BarPolarityStreak::new("bps");
        let v = bps.update_bar(&bar("100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bps_consecutive_bullish_increments() {
        let mut bps = BarPolarityStreak::new("bps");
        bps.update_bar(&bar("100", "105")).unwrap();
        bps.update_bar(&bar("105", "110")).unwrap();
        let v = bps.update_bar(&bar("110", "115")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_bps_polarity_flip_resets_to_one() {
        let mut bps = BarPolarityStreak::new("bps");
        bps.update_bar(&bar("100", "105")).unwrap();
        bps.update_bar(&bar("105", "110")).unwrap();
        // Now bearish — should reset
        let v = bps.update_bar(&bar("110", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bps_neutral_breaks_streak() {
        let mut bps = BarPolarityStreak::new("bps");
        bps.update_bar(&bar("100", "105")).unwrap();
        // Neutral bar (open == close)
        let v = bps.update_bar(&bar("105", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bps_reset() {
        let mut bps = BarPolarityStreak::new("bps");
        bps.update_bar(&bar("100", "105")).unwrap();
        assert!(bps.is_ready());
        bps.reset();
        assert!(!bps.is_ready());
    }
}
