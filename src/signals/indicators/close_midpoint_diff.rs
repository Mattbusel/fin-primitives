//! Close-Midpoint Difference — how far the close is from the bar midpoint.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close-Midpoint Difference — `close - (high + low) / 2`.
///
/// Measures where the close lands relative to the center of the bar's range:
/// - **Positive**: close is above the midpoint (bullish close within the bar).
/// - **Negative**: close is below the midpoint (bearish close within the bar).
/// - **Zero**: close exactly at the midpoint.
///
/// This is a period-1 indicator that emits on every bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseMidpointDiff;
/// use fin_primitives::signals::Signal;
/// let cmd = CloseMidpointDiff::new("cmd");
/// assert_eq!(cmd.period(), 1);
/// ```
pub struct CloseMidpointDiff {
    name: String,
}

impl CloseMidpointDiff {
    /// Constructs a new `CloseMidpointDiff`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for CloseMidpointDiff {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let midpoint = (bar.high + bar.low)
            .checked_div(Decimal::from(2u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(bar.close - midpoint))
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cmd_close_at_high() {
        let mut cmd = CloseMidpointDiff::new("cmd");
        // high=110, low=90, close=110 → midpoint=100, diff=10
        let v = cmd.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_cmd_close_at_low() {
        let mut cmd = CloseMidpointDiff::new("cmd");
        // high=110, low=90, close=90 → midpoint=100, diff=-10
        let v = cmd.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_cmd_close_at_midpoint() {
        let mut cmd = CloseMidpointDiff::new("cmd");
        // high=110, low=90, close=100 → midpoint=100, diff=0
        let v = cmd.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cmd_always_ready() {
        let cmd = CloseMidpointDiff::new("cmd");
        assert!(cmd.is_ready());
    }

    #[test]
    fn test_cmd_period_and_name() {
        let cmd = CloseMidpointDiff::new("my_cmd");
        assert_eq!(cmd.period(), 1);
        assert_eq!(cmd.name(), "my_cmd");
    }
}
