//! Close Retrace Percent indicator -- how far close retraced from the bar high.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close Retrace Percent -- measures how far the close has retraced from the bar's high
/// within the bar's total range.
///
/// ```text
/// retrace[t] = (high - close) / (high - low) x 100
/// ```
///
/// Interpretation:
/// - 0%   → close == high (fully bullish bar)
/// - 50%  → close at midpoint of range
/// - 100% → close == low (fully bearish bar)
///
/// Returns [`SignalValue::Unavailable`] if `high == low` (zero-range doji).
/// Becomes ready after the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseRetracePct;
/// use fin_primitives::signals::Signal;
/// let crp = CloseRetracePct::new("crp");
/// assert_eq!(crp.period(), 1);
/// ```
pub struct CloseRetracePct {
    name: String,
    ready: bool,
}

impl CloseRetracePct {
    /// Constructs a new `CloseRetracePct`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ready: false }
    }
}

impl Signal for CloseRetracePct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.ready }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.ready = true;
        let range = bar.high - bar.low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        let retrace = (bar.high - bar.close) / range * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(retrace))
    }

    fn reset(&mut self) {
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_crp_close_at_high_is_zero() {
        let mut crp = CloseRetracePct::new("crp");
        let v = crp.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_crp_close_at_low_is_100() {
        let mut crp = CloseRetracePct::new("crp");
        let v = crp.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_crp_close_at_midpoint_is_50() {
        let mut crp = CloseRetracePct::new("crp");
        let v = crp.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_crp_zero_range_unavailable() {
        let mut crp = CloseRetracePct::new("crp");
        let v = crp.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_crp_ready_after_first_bar() {
        let mut crp = CloseRetracePct::new("crp");
        assert!(!crp.is_ready());
        crp.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(crp.is_ready());
    }

    #[test]
    fn test_crp_reset() {
        let mut crp = CloseRetracePct::new("crp");
        crp.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(crp.is_ready());
        crp.reset();
        assert!(!crp.is_ready());
    }
}
