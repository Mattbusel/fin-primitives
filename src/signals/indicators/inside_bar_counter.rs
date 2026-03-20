//! Inside Bar Counter — counts consecutive inside bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Inside Bar Counter — counts consecutive inside bars.
///
/// An **inside bar** is a bar whose high is less than or equal to the previous bar's high
/// *and* whose low is greater than or equal to the previous bar's low. The bar's entire
/// range is "inside" the prior bar.
///
/// - **Positive** output: the current run of consecutive inside bars.
/// - **Zero**: the current bar is not an inside bar (it broke the prior range).
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no previous bar to compare).
///
/// Consecutive inside bars represent price compression. A sequence of 3+ inside bars
/// often precedes a volatility expansion or breakout.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::InsideBarCounter;
/// use fin_primitives::signals::Signal;
/// let ibc = InsideBarCounter::new("ibc");
/// assert_eq!(ibc.period(), 1);
/// ```
pub struct InsideBarCounter {
    name: String,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    streak: u32,
}

impl InsideBarCounter {
    /// Constructs a new `InsideBarCounter`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prev_high: None,
            prev_low: None,
            streak: 0,
        }
    }
}

impl Signal for InsideBarCounter {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        self.prev_high.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) else {
            self.prev_high = Some(bar.high);
            self.prev_low = Some(bar.low);
            return Ok(SignalValue::Unavailable);
        };

        let is_inside = bar.high <= ph && bar.low >= pl;
        if is_inside {
            self.streak += 1;
        } else {
            self.streak = 0;
        }

        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);

        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.prev_high = None;
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
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
    fn test_ibc_unavailable_on_first_bar() {
        let mut ibc = InsideBarCounter::new("ibc");
        assert_eq!(ibc.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(ibc.is_ready()); // prev bar is stored, next call will yield a scalar
    }

    #[test]
    fn test_ibc_outside_bar_gives_zero() {
        let mut ibc = InsideBarCounter::new("ibc");
        ibc.update_bar(&bar("110", "90")).unwrap();
        // Outside bar: high > prev_high
        let v = ibc.update_bar(&bar("120", "85")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ibc_inside_bar_gives_one() {
        let mut ibc = InsideBarCounter::new("ibc");
        ibc.update_bar(&bar("110", "90")).unwrap();
        // Inside bar: high <= 110 and low >= 90
        let v = ibc.update_bar(&bar("105", "95")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ibc_consecutive_inside_bars_increments() {
        let mut ibc = InsideBarCounter::new("ibc");
        ibc.update_bar(&bar("110", "90")).unwrap();
        ibc.update_bar(&bar("108", "92")).unwrap(); // inside → streak=1
        let v = ibc.update_bar(&bar("106", "94")).unwrap(); // inside inside → streak=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_ibc_outside_bar_resets_streak() {
        let mut ibc = InsideBarCounter::new("ibc");
        ibc.update_bar(&bar("110", "90")).unwrap();
        ibc.update_bar(&bar("108", "92")).unwrap(); // streak=1
        ibc.update_bar(&bar("106", "94")).unwrap(); // streak=2
        let v = ibc.update_bar(&bar("120", "80")).unwrap(); // outside → streak=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ibc_ready_after_first_bar() {
        let mut ibc = InsideBarCounter::new("ibc");
        assert!(!ibc.is_ready());
        ibc.update_bar(&bar("110", "90")).unwrap(); // prev stored → ready for next update
        assert!(ibc.is_ready());
    }

    #[test]
    fn test_ibc_reset() {
        let mut ibc = InsideBarCounter::new("ibc");
        ibc.update_bar(&bar("110", "90")).unwrap();
        ibc.update_bar(&bar("108", "92")).unwrap();
        assert!(ibc.is_ready());
        ibc.reset();
        assert!(!ibc.is_ready());
        assert_eq!(ibc.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ibc_period_and_name() {
        let ibc = InsideBarCounter::new("my_ibc");
        assert_eq!(ibc.period(), 1);
        assert_eq!(ibc.name(), "my_ibc");
    }
}
