//! Relative Close indicator — close position within the bar's range, as a percentage.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Relative Close — where the close falls within the bar's high-low range, as a percentage.
///
/// ```text
/// relative_close[t] = (close - low) / (high - low) × 100
/// ```
///
/// - 100 % means close == high (strongest close).
/// - 0 % means close == low (weakest close).
/// - 50 % means close at midpoint.
///
/// Returns [`SignalValue::Unavailable`] when `high == low` (zero-range bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RelativeClose;
/// use fin_primitives::signals::Signal;
/// let rc = RelativeClose::new("rc");
/// assert_eq!(rc.period(), 1);
/// ```
pub struct RelativeClose {
    name: String,
}

impl RelativeClose {
    /// Constructs a new `RelativeClose`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for RelativeClose {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        let rc = (bar.close - bar.low) / range * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(rc))
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
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rc_close_at_high() {
        let mut rc = RelativeClose::new("rc");
        let v = rc.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_rc_close_at_low() {
        let mut rc = RelativeClose::new("rc");
        let v = rc.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rc_close_at_midpoint() {
        let mut rc = RelativeClose::new("rc");
        let v = rc.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_rc_zero_range_unavailable() {
        let mut rc = RelativeClose::new("rc");
        let v = rc.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rc_always_ready() {
        let rc = RelativeClose::new("rc");
        assert!(rc.is_ready());
    }
}
