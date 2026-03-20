//! Close Midpoint Strength — signed measure of where the close sits relative to bar midpoint.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close Midpoint Strength — `(2 × close - high - low) / (high - low)`.
///
/// A symmetric measure of the close's position relative to the bar midpoint:
/// - **+1.0**: close equals the high (maximum bullish close).
/// - **−1.0**: close equals the low (maximum bearish close).
/// - **0.0**: close exactly at the midpoint.
///
/// Equivalent to `2 × close_location_value - 1`.
///
/// Returns [`SignalValue::Unavailable`] when the bar range is zero (flat bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseMidpointStrength;
/// use fin_primitives::signals::Signal;
/// let cms = CloseMidpointStrength::new("cms");
/// assert_eq!(cms.period(), 1);
/// ```
pub struct CloseMidpointStrength {
    name: String,
}

impl CloseMidpointStrength {
    /// Constructs a new `CloseMidpointStrength`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for CloseMidpointStrength {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        // (2*close - high - low) / range
        let numerator = bar.close * Decimal::TWO - bar.high - bar.low;
        let strength = numerator
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(strength.clamp(Decimal::NEGATIVE_ONE, Decimal::ONE)))
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
    fn test_cms_close_at_high_gives_one() {
        let mut s = CloseMidpointStrength::new("cms");
        let v = s.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cms_close_at_low_gives_neg_one() {
        let mut s = CloseMidpointStrength::new("cms");
        let v = s.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_cms_close_at_midpoint_gives_zero() {
        let mut s = CloseMidpointStrength::new("cms");
        let v = s.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cms_flat_bar_unavailable() {
        let mut s = CloseMidpointStrength::new("cms");
        assert_eq!(s.update_bar(&bar("100", "100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cms_output_in_range() {
        let mut s = CloseMidpointStrength::new("cms");
        for (h, l, c) in &[("110","90","105"), ("115","85","98")] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(h, l, c)).unwrap() {
                assert!(v >= dec!(-1) && v <= dec!(1), "out of [-1,1]: {v}");
            }
        }
    }

    #[test]
    fn test_cms_always_ready() {
        let s = CloseMidpointStrength::new("cms");
        assert!(s.is_ready());
        assert_eq!(s.period(), 1);
    }
}
