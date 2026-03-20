//! Body Position indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Body Position — where the candle body sits within the bar's high-low range, as a fraction.
///
/// ```text
/// body_midpoint = (max(open, close) + min(open, close)) / 2
///               = (body_high + body_low) / 2
/// position      = (body_midpoint - low) / (high - low)
/// ```
///
/// - **Near 1.0**: body floats at the top of the bar — bullish close relative to range.
/// - **Near 0.0**: body sinks to the bottom — bearish close relative to range.
/// - **0.5**: body centered in the range.
/// - Returns `0.5` when the bar has no range (`high == low`).
/// - Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyPosition;
/// use fin_primitives::signals::Signal;
///
/// let bp = BodyPosition::new("bp").unwrap();
/// assert_eq!(bp.period(), 1);
/// ```
pub struct BodyPosition {
    name: String,
}

impl BodyPosition {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for BodyPosition {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::new(5, 1))); // 0.5
        }
        let two = Decimal::TWO;
        let body_mid = (bar.body_high() + bar.body_low()) / two;
        let position = (body_mid - bar.low) / range;
        Ok(SignalValue::Scalar(position))
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
    fn test_bp_always_ready() {
        let bp = BodyPosition::new("bp").unwrap();
        assert!(bp.is_ready());
    }

    #[test]
    fn test_bp_no_range_returns_half() {
        let mut bp = BodyPosition::new("bp").unwrap();
        let result = bp.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_bp_body_at_top() {
        // high=110, low=90, body from 100..110 → midpoint=105, position=(105-90)/20=0.75
        let mut bp = BodyPosition::new("bp").unwrap();
        let result = bp.update_bar(&bar("100", "110", "90", "110")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.75)));
    }

    #[test]
    fn test_bp_body_at_bottom() {
        // high=110, low=90, body from 90..100 → midpoint=95, position=(95-90)/20=0.25
        let mut bp = BodyPosition::new("bp").unwrap();
        let result = bp.update_bar(&bar("100", "110", "90", "90")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.25)));
    }

    #[test]
    fn test_bp_body_at_center() {
        // high=110, low=90, body from 95..105 → midpoint=100, position=(100-90)/20=0.5
        let mut bp = BodyPosition::new("bp").unwrap();
        let result = bp.update_bar(&bar("95", "110", "90", "105")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.5)));
    }
}
