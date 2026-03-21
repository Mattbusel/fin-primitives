//! Gap Direction Streak indicator.
//!
//! Counts consecutive bars where the opening gap direction matches the
//! prior bar's gap direction, detecting persistent overnight momentum.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Gap Direction Streak — count of bars in the current unbroken run of
/// same-direction opening gaps.
///
/// Each bar's gap direction is `sign(open - prev_close)`:
/// - **+1**: gap up
/// - **-1**: gap down
/// - **0**: flat open (no gap)
///
/// The streak increments when the current gap direction matches the previous
/// gap direction. It resets to 1 on any change (direction flip or flat open).
/// Flat opens (`direction == 0`) also reset the streak to 0.
///
/// A high streak means the market is repeatedly opening in the same direction,
/// indicating sustained overnight momentum bias.
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no previous close).
///
/// # Errors
/// Never fails; `new` always returns `Ok`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GapDirectionStreak;
/// use fin_primitives::signals::Signal;
/// let gds = GapDirectionStreak::new("gds").unwrap();
/// assert_eq!(gds.period(), 1);
/// ```
pub struct GapDirectionStreak {
    name: String,
    streak: u32,
    last_direction: i8,
    prev_close: Option<Decimal>,
    seen_bars: usize,
}

impl GapDirectionStreak {
    /// Constructs a new `GapDirectionStreak`.
    ///
    /// # Errors
    /// Never fails.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), streak: 0, last_direction: 0, prev_close: None, seen_bars: 0 })
    }
}

impl Signal for GapDirectionStreak {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.seen_bars >= 2 }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.seen_bars += 1;
        let pc = self.prev_close;
        self.prev_close = Some(bar.close);

        let Some(prev_close) = pc else {
            return Ok(SignalValue::Unavailable);
        };

        let direction: i8 = if bar.open > prev_close {
            1
        } else if bar.open < prev_close {
            -1
        } else {
            0
        };

        if direction == 0 || direction != self.last_direction {
            self.streak = if direction == 0 { 0 } else { 1 };
        } else {
            self.streak += 1;
        }
        self.last_direction = direction;

        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.streak = 0;
        self.last_direction = 0;
        self.prev_close = None;
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

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let high = if cp > op { cp } else { op };
        let low = if cp < op { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high, low, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_gds_first_bar_unavailable() {
        let mut gds = GapDirectionStreak::new("gds").unwrap();
        assert_eq!(gds.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
        assert!(!gds.is_ready());
    }

    #[test]
    fn test_gds_ready_after_second_bar() {
        let mut gds = GapDirectionStreak::new("gds").unwrap();
        gds.update_bar(&bar("100", "102")).unwrap(); // bar 1: Unavailable
        gds.update_bar(&bar("104", "106")).unwrap(); // bar 2: Scalar → ready
        assert!(gds.is_ready());
    }

    #[test]
    fn test_gds_first_gap_up_gives_one() {
        let mut gds = GapDirectionStreak::new("gds").unwrap();
        gds.update_bar(&bar("100", "102")).unwrap(); // close=102
        if let SignalValue::Scalar(v) = gds.update_bar(&bar("105", "107")).unwrap() {
            // open=105 > prev_close=102 → gap up, first up → streak=1
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gds_consecutive_gap_ups_increment() {
        let mut gds = GapDirectionStreak::new("gds").unwrap();
        gds.update_bar(&bar("100", "100")).unwrap();
        gds.update_bar(&bar("102", "102")).unwrap(); // gap up → 1
        gds.update_bar(&bar("104", "104")).unwrap(); // gap up → 2
        if let SignalValue::Scalar(v) = gds.update_bar(&bar("106", "106")).unwrap() {
            assert_eq!(v, dec!(3));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gds_direction_change_resets_to_one() {
        let mut gds = GapDirectionStreak::new("gds").unwrap();
        gds.update_bar(&bar("100", "100")).unwrap();
        gds.update_bar(&bar("102", "102")).unwrap(); // gap up → 1
        gds.update_bar(&bar("104", "104")).unwrap(); // gap up → 2
        if let SignalValue::Scalar(v) = gds.update_bar(&bar("101", "101")).unwrap() {
            // gap down (101 < 104) → direction changed → streak = 1
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gds_flat_open_resets_to_zero() {
        let mut gds = GapDirectionStreak::new("gds").unwrap();
        gds.update_bar(&bar("100", "100")).unwrap();
        gds.update_bar(&bar("102", "102")).unwrap(); // gap up → 1
        if let SignalValue::Scalar(v) = gds.update_bar(&bar("102", "103")).unwrap() {
            // open=102 == prev_close=102 → flat → streak=0
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gds_period_is_one() {
        let gds = GapDirectionStreak::new("gds").unwrap();
        assert_eq!(gds.period(), 1);
    }

    #[test]
    fn test_gds_reset() {
        let mut gds = GapDirectionStreak::new("gds").unwrap();
        gds.update_bar(&bar("100", "102")).unwrap();
        gds.update_bar(&bar("104", "106")).unwrap();
        assert!(gds.is_ready());
        gds.reset();
        assert!(!gds.is_ready());
        assert_eq!(gds.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
    }
}
