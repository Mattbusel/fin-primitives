//! Gap Streak — signed count of consecutive opening gaps in the same direction.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Gap Streak — signed count of consecutive directional opening gaps.
///
/// Tracks how many bars in a row have opened above or below the prior close:
/// - **Positive N**: `open > prev_close` for the last N consecutive bars — sustained gap-up momentum.
/// - **Negative N**: `open < prev_close` for the last N consecutive bars — sustained gap-down pressure.
/// - **0**: the current bar opened flat (at exactly the prior close) — gap streak broken.
///
/// Returns [`SignalValue::Unavailable`] for the first bar (no prior close).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0` (period is used as required for
/// the [`Signal`] trait; conceptually this indicator is always period=1).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GapStreak;
/// use fin_primitives::signals::Signal;
/// let gs = GapStreak::new("gap_streak").unwrap();
/// assert_eq!(gs.period(), 1);
/// ```
pub struct GapStreak {
    name: String,
    streak: i64,
    prev_close: Option<Decimal>,
    ready: bool,
}

impl GapStreak {
    /// Constructs a new `GapStreak`.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            streak: 0,
            prev_close: None,
            ready: false,
        })
    }
}

impl Signal for GapStreak {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.ready }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(pc) = self.prev_close {
            if bar.open > pc {
                // Gap up — extend up-streak or start new
                if self.streak > 0 {
                    self.streak += 1;
                } else {
                    self.streak = 1;
                }
            } else if bar.open < pc {
                // Gap down — extend down-streak or start new
                if self.streak < 0 {
                    self.streak -= 1;
                } else {
                    self.streak = -1;
                }
            } else {
                // Flat open — reset streak
                self.streak = 0;
            }
            SignalValue::Scalar(Decimal::from(self.streak))
        } else {
            SignalValue::Unavailable
        };

        self.prev_close = Some(bar.close);
        self.ready = true;
        Ok(result)
    }

    fn reset(&mut self) {
        self.streak = 0;
        self.prev_close = None;
        self.ready = false;
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
        let hp = if cp > op { cp } else { op };
        let lp = if cp < op { cp } else { op };
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
    fn test_gs_first_bar_unavailable() {
        let mut s = GapStreak::new("gs").unwrap();
        assert!(!s.is_ready());
        assert_eq!(s.update_bar(&bar("100","100")).unwrap(), SignalValue::Unavailable);
        assert!(s.is_ready()); // ready after first bar (prev_close is set)
    }

    #[test]
    fn test_gs_consecutive_gap_ups() {
        let mut s = GapStreak::new("gs").unwrap();
        s.update_bar(&bar("100","100")).unwrap(); // base bar, close=100
        // Next bar opens at 102 (> prev_close=100) → gap up streak=1
        let v1 = s.update_bar(&bar("102","103")).unwrap();
        // Next bar opens at 105 (> prev_close=103) → gap up streak=2
        let v2 = s.update_bar(&bar("105","106")).unwrap();
        assert_eq!(v1, SignalValue::Scalar(dec!(1)));
        assert_eq!(v2, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_gs_consecutive_gap_downs() {
        let mut s = GapStreak::new("gs").unwrap();
        s.update_bar(&bar("100","100")).unwrap(); // base bar, close=100
        let v1 = s.update_bar(&bar("98","97")).unwrap();  // gap down
        let v2 = s.update_bar(&bar("95","94")).unwrap();  // gap down again
        assert_eq!(v1, SignalValue::Scalar(dec!(-1)));
        assert_eq!(v2, SignalValue::Scalar(dec!(-2)));
    }

    #[test]
    fn test_gs_streak_resets_on_direction_change() {
        let mut s = GapStreak::new("gs").unwrap();
        s.update_bar(&bar("100","100")).unwrap();
        s.update_bar(&bar("102","103")).unwrap(); // gap up, streak=1
        // Gap down after up-streak → resets to -1
        if let SignalValue::Scalar(v) = s.update_bar(&bar("101","100")).unwrap() {
            assert_eq!(v, dec!(-1), "direction change resets streak to -1");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gs_flat_open_gives_zero() {
        let mut s = GapStreak::new("gs").unwrap();
        s.update_bar(&bar("100","105")).unwrap(); // close=105
        if let SignalValue::Scalar(v) = s.update_bar(&bar("105","106")).unwrap() {
            // open=105 = prev_close=105 → streak=0
            assert_eq!(v, dec!(0), "flat open gives streak=0");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gs_reset() {
        let mut s = GapStreak::new("gs").unwrap();
        for (o, c) in &[("100","100"),("102","103"),("105","106")] {
            s.update_bar(&bar(o, c)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
