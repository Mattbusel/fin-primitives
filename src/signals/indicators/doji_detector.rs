//! Doji Detector indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Doji Detector — identifies doji candlestick patterns where the body is very
/// small relative to the total range, signaling indecision.
///
/// A bar is classified as a doji when:
/// ```text
/// |close - open| / (high - low) < threshold
/// ```
///
/// Outputs:
/// - `1` → doji detected
/// - `0` → not a doji (or zero-range bar)
///
/// Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DojiDetector;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
///
/// let d = DojiDetector::new("doji", dec!(0.1)).unwrap();
/// assert_eq!(d.period(), 1);
/// ```
pub struct DojiDetector {
    name: String,
    threshold: Decimal,
}

impl DojiDetector {
    /// Constructs a new `DojiDetector`.
    ///
    /// `threshold` is the maximum body-to-range ratio for doji classification.
    /// A typical value is `0.1` (body ≤ 10% of total range).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `threshold` is not in (0, 1].
    pub fn new(name: impl Into<String>, threshold: Decimal) -> Result<Self, FinError> {
        if threshold <= Decimal::ZERO || threshold > Decimal::ONE {
            return Err(FinError::InvalidInput("threshold must be in (0, 1]".into()));
        }
        Ok(Self { name: name.into(), threshold })
    }
}

impl Signal for DojiDetector {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let body = bar.body_size();
        let ratio = body / range;
        let is_doji = ratio < self.threshold;
        Ok(SignalValue::Scalar(if is_doji { Decimal::ONE } else { Decimal::ZERO }))
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
    fn test_doji_invalid_threshold() {
        assert!(DojiDetector::new("d", dec!(0)).is_err());
        assert!(DojiDetector::new("d", dec!(1.1)).is_err());
    }

    #[test]
    fn test_doji_detects_doji() {
        let mut d = DojiDetector::new("d", dec!(0.1)).unwrap();
        // body = |100.5 - 100| = 0.5, range = 110 - 90 = 20, ratio = 0.025 < 0.1
        let result = d.update_bar(&bar("100", "110", "90", "100.5")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_doji_rejects_non_doji() {
        let mut d = DojiDetector::new("d", dec!(0.1)).unwrap();
        // body = |110 - 90| = 20, range = 20, ratio = 1.0 ≥ 0.1
        let result = d.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_doji_zero_range() {
        let mut d = DojiDetector::new("d", dec!(0.1)).unwrap();
        let result = d.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_doji_always_ready() {
        let d = DojiDetector::new("d", dec!(0.1)).unwrap();
        assert!(d.is_ready());
    }
}
