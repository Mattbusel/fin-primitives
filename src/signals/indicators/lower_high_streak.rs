//! Lower-High Streak — count of consecutive bars with a lower high than the previous bar.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Lower-High Streak — count of consecutive bars where `high < prev_high`.
///
/// Resets to 0 whenever the current bar makes a new high or matches the prior high.
/// Useful for detecting sustained resistance and descending highs as a bearish trend
/// confirmation signal:
/// - **High streak**: successive lower highs — weakening upside momentum.
/// - **0**: streak broken — current bar made a new high.
///
/// Returns [`SignalValue::Unavailable`] until 2 bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LowerHighStreak;
/// use fin_primitives::signals::Signal;
/// let lhs = LowerHighStreak::new("lhs");
/// assert_eq!(lhs.period(), 1);
/// ```
pub struct LowerHighStreak {
    name: String,
    prev_high: Option<Decimal>,
    streak: u32,
}

impl LowerHighStreak {
    /// Constructs a new `LowerHighStreak`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_high: None, streak: 0 }
    }
}

impl Signal for LowerHighStreak {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_high.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(ph) = self.prev_high {
            if bar.high < ph {
                self.streak += 1;
            } else {
                self.streak = 0;
            }
            Ok(SignalValue::Scalar(Decimal::from(self.streak)))
        } else {
            Ok(SignalValue::Unavailable)
        };

        self.prev_high = Some(bar.high);
        result
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

    fn bar(h: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(
            (hp.value() - Decimal::TEN).to_string().parse().unwrap()
        ).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_lhs_first_bar_unavailable() {
        let mut s = LowerHighStreak::new("lhs");
        assert_eq!(s.update_bar(&bar("110")).unwrap(), SignalValue::Unavailable);
        assert!(s.is_ready());
    }

    #[test]
    fn test_lhs_descending_highs() {
        let mut s = LowerHighStreak::new("lhs");
        s.update_bar(&bar("110")).unwrap();
        assert_eq!(s.update_bar(&bar("108")).unwrap(), SignalValue::Scalar(dec!(1)));
        assert_eq!(s.update_bar(&bar("106")).unwrap(), SignalValue::Scalar(dec!(2)));
        assert_eq!(s.update_bar(&bar("104")).unwrap(), SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_lhs_streak_resets_on_higher_high() {
        let mut s = LowerHighStreak::new("lhs");
        s.update_bar(&bar("110")).unwrap();
        s.update_bar(&bar("108")).unwrap(); // streak=1
        s.update_bar(&bar("106")).unwrap(); // streak=2
        let v = s.update_bar(&bar("112")).unwrap(); // new high → reset to 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_lhs_equal_high_resets() {
        let mut s = LowerHighStreak::new("lhs");
        s.update_bar(&bar("110")).unwrap();
        s.update_bar(&bar("108")).unwrap(); // streak=1
        let v = s.update_bar(&bar("108")).unwrap(); // same high → not lower → reset
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_lhs_reset() {
        let mut s = LowerHighStreak::new("lhs");
        s.update_bar(&bar("110")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
