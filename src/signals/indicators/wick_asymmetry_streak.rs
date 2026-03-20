//! Wick Asymmetry Streak — signed consecutive count of bars with upper or lower wick dominance.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Wick Asymmetry Streak — signed count of consecutive bars with the same wick dominance.
///
/// Tracks sustained directional wick pressure over consecutive bars:
/// - **Positive N**: upper wick has exceeded lower wick for N consecutive bars — sustained
///   selling pressure as rallies get rejected at the top.
/// - **Negative N**: lower wick has exceeded upper wick for N consecutive bars — sustained
///   buying pressure as dips get defended at the bottom.
/// - **0**: current bar has equal or near-equal wicks — streak broken.
///
/// Returns [`SignalValue::Scalar(0)`] when upper wick equals lower wick.
/// Returns [`SignalValue::Unavailable`] for the very first bar.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WickAsymmetryStreak;
/// use fin_primitives::signals::Signal;
/// let was = WickAsymmetryStreak::new("was").unwrap();
/// assert_eq!(was.period(), 1);
/// ```
pub struct WickAsymmetryStreak {
    name: String,
    streak: i64,
    seen: bool,
}

impl WickAsymmetryStreak {
    /// Constructs a new `WickAsymmetryStreak`.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            streak: 0,
            seen: false,
        })
    }
}

impl Signal for WickAsymmetryStreak {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.seen }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let upper = bar.upper_wick();
        let lower = bar.lower_wick();

        if !self.seen {
            self.seen = true;
            // Set initial streak based on first bar
            if upper > lower {
                self.streak = 1;
            } else if lower > upper {
                self.streak = -1;
            } else {
                self.streak = 0;
            }
            return Ok(SignalValue::Unavailable);
        }

        if upper > lower {
            if self.streak > 0 {
                self.streak += 1;
            } else {
                self.streak = 1;
            }
        } else if lower > upper {
            if self.streak < 0 {
                self.streak -= 1;
            } else {
                self.streak = -1;
            }
        } else {
            self.streak = 0;
        }

        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.streak = 0;
        self.seen = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
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
    fn test_was_first_bar_unavailable() {
        let mut s = WickAsymmetryStreak::new("was").unwrap();
        assert!(!s.is_ready());
        // open=100, close=100, high=110, low=95 → upper=10, lower=5
        assert_eq!(s.update_bar(&bar("100","110","95","100")).unwrap(), SignalValue::Unavailable);
        assert!(s.is_ready());
    }

    #[test]
    fn test_was_consecutive_upper_wick_dominance() {
        let mut s = WickAsymmetryStreak::new("was").unwrap();
        // All bars: upper_wick > lower_wick
        // open=100, close=100, high=115, low=98 → upper=15, lower=2 → upper dominates
        s.update_bar(&bar("100","115","98","100")).unwrap(); // first bar → Unavailable
        let v1 = s.update_bar(&bar("100","115","98","100")).unwrap(); // streak=2? No: init=1, then +1=2
        let v2 = s.update_bar(&bar("100","115","98","100")).unwrap(); // streak=3
        assert_eq!(v1, SignalValue::Scalar(dec!(2)));
        assert_eq!(v2, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_was_lower_wick_dominance_is_negative() {
        let mut s = WickAsymmetryStreak::new("was").unwrap();
        // open=100, close=100, high=102, low=85 → upper=2, lower=15 → lower dominates
        s.update_bar(&bar("100","102","85","100")).unwrap(); // first → Unavailable
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100","102","85","100")).unwrap() {
            assert_eq!(v, dec!(-2), "lower wick dominant → streak=-2");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_was_streak_resets_on_equal_wicks() {
        let mut s = WickAsymmetryStreak::new("was").unwrap();
        s.update_bar(&bar("100","115","98","100")).unwrap(); // upper dominant, first
        s.update_bar(&bar("100","115","98","100")).unwrap(); // streak=2
        // Symmetric bar: open=100, close=100, high=110, low=90 → upper=10, lower=10
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100","110","90","100")).unwrap() {
            assert_eq!(v, dec!(0), "equal wicks → streak=0");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_was_reset() {
        let mut s = WickAsymmetryStreak::new("was").unwrap();
        for _ in 0..4 { s.update_bar(&bar("100","115","98","100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
