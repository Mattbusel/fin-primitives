//! Higher-Low Streak — count of consecutive bars with a higher low than the previous bar.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Higher-Low Streak — count of consecutive bars where `low > prev_low`.
///
/// Resets to 0 whenever the current bar has a lower low. Useful for detecting
/// sustained support-building (ascending lows) as a trend confirmation signal:
/// - **High streak**: strong bullish structure — each dip is shallower.
/// - **0**: streak broken — current bar made a lower low.
///
/// Returns [`SignalValue::Unavailable`] until 2 bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HigherLowStreak;
/// use fin_primitives::signals::Signal;
/// let hls = HigherLowStreak::new("hls");
/// assert_eq!(hls.period(), 1);
/// ```
pub struct HigherLowStreak {
    name: String,
    prev_low: Option<Decimal>,
    streak: u32,
}

impl HigherLowStreak {
    /// Constructs a new `HigherLowStreak`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_low: None, streak: 0 }
    }
}

impl Signal for HigherLowStreak {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_low.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(pl) = self.prev_low {
            if bar.low > pl {
                self.streak += 1;
            } else {
                self.streak = 0;
            }
            Ok(SignalValue::Scalar(Decimal::from(self.streak)))
        } else {
            Ok(SignalValue::Unavailable)
        };

        self.prev_low = Some(bar.low);
        result
    }

    fn reset(&mut self) {
        self.prev_low = None;
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

    fn bar(l: &str) -> OhlcvBar {
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let hp = Price::new((lp.value() + Decimal::TEN).to_string().parse().unwrap()).unwrap();
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
    fn test_hls_first_bar_unavailable() {
        let mut s = HigherLowStreak::new("hls");
        assert_eq!(s.update_bar(&bar("90")).unwrap(), SignalValue::Unavailable);
        assert!(s.is_ready());
    }

    #[test]
    fn test_hls_ascending_lows() {
        let mut s = HigherLowStreak::new("hls");
        s.update_bar(&bar("90")).unwrap();
        assert_eq!(s.update_bar(&bar("92")).unwrap(), SignalValue::Scalar(dec!(1)));
        assert_eq!(s.update_bar(&bar("94")).unwrap(), SignalValue::Scalar(dec!(2)));
        assert_eq!(s.update_bar(&bar("96")).unwrap(), SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_hls_streak_resets_on_lower_low() {
        let mut s = HigherLowStreak::new("hls");
        s.update_bar(&bar("90")).unwrap();
        s.update_bar(&bar("92")).unwrap(); // streak=1
        s.update_bar(&bar("94")).unwrap(); // streak=2
        let v = s.update_bar(&bar("91")).unwrap(); // lower low → reset to 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hls_equal_low_resets() {
        let mut s = HigherLowStreak::new("hls");
        s.update_bar(&bar("90")).unwrap();
        s.update_bar(&bar("92")).unwrap(); // streak=1
        let v = s.update_bar(&bar("92")).unwrap(); // same low → not higher → reset
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hls_reset() {
        let mut s = HigherLowStreak::new("hls");
        s.update_bar(&bar("90")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
