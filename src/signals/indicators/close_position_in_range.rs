//! Close Position in Range indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close Position in Range — measures where the close falls within the bar's
/// high-low range as a value in [0, 1].
///
/// ```text
/// CPR = (close - low) / (high - low)
/// ```
///
/// - `1.0` → close at the high  
/// - `0.5` → close at the midpoint  
/// - `0.0` → close at the low  
///
/// Returns `0.5` when the bar has no range (`high == low`).
/// Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ClosePositionInRange;
/// use fin_primitives::signals::Signal;
///
/// let cpr = ClosePositionInRange::new("cpr").unwrap();
/// assert_eq!(cpr.period(), 1);
/// ```
pub struct ClosePositionInRange {
    name: String,
}

impl ClosePositionInRange {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for ClosePositionInRange {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::new(5, 1))); // 0.5
        }
        Ok(SignalValue::Scalar((bar.close - bar.low) / range))
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
    fn test_cpr_always_ready() {
        let cpr = ClosePositionInRange::new("cpr").unwrap();
        assert!(cpr.is_ready());
    }

    #[test]
    fn test_cpr_no_range() {
        let mut cpr = ClosePositionInRange::new("cpr").unwrap();
        assert_eq!(cpr.update_bar(&bar("100", "100", "100")).unwrap(), SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_cpr_close_at_high() {
        let mut cpr = ClosePositionInRange::new("cpr").unwrap();
        assert_eq!(cpr.update_bar(&bar("110", "90", "110")).unwrap(), SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cpr_close_at_low() {
        let mut cpr = ClosePositionInRange::new("cpr").unwrap();
        assert_eq!(cpr.update_bar(&bar("110", "90", "90")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cpr_close_at_midpoint() {
        let mut cpr = ClosePositionInRange::new("cpr").unwrap();
        assert_eq!(cpr.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Scalar(dec!(0.5)));
    }
}
