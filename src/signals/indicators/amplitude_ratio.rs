//! Amplitude Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Amplitude Ratio — the bar's high-low range normalized by the previous close.
///
/// ```text
/// amplitude_ratio = (high - low) / prev_close × 100
/// ```
///
/// This provides a percentage-based measure of intrabar volatility relative to
/// price level, making it comparable across different price points.
///
/// Returns [`SignalValue::Unavailable`] on the first bar or if prev_close is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AmplitudeRatio;
/// use fin_primitives::signals::Signal;
///
/// let ar = AmplitudeRatio::new("ar").unwrap();
/// assert_eq!(ar.period(), 1);
/// ```
pub struct AmplitudeRatio {
    name: String,
    prev_close: Option<Decimal>,
}

impl AmplitudeRatio {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_close: None })
    }
}

impl Signal for AmplitudeRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) if pc.is_zero() => SignalValue::Unavailable,
            Some(pc) => {
                let range = bar.high - bar.low;
                SignalValue::Scalar(range / pc * Decimal::ONE_HUNDRED)
            }
        };
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
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
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ar_unavailable_first_bar() {
        let mut ar = AmplitudeRatio::new("ar").unwrap();
        assert_eq!(ar.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ar_10_pct_range() {
        let mut ar = AmplitudeRatio::new("ar").unwrap();
        ar.update_bar(&bar("110", "90", "100")).unwrap(); // prev_close = 100
        // range = 110-90 = 20, prev_close = 100 → 20/100*100 = 20%
        let result = ar.update_bar(&bar("110", "90", "105")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_ar_zero_range() {
        let mut ar = AmplitudeRatio::new("ar").unwrap();
        ar.update_bar(&bar("100", "100", "100")).unwrap();
        let result = ar.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ar_reset() {
        let mut ar = AmplitudeRatio::new("ar").unwrap();
        ar.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(ar.is_ready());
        ar.reset();
        assert!(!ar.is_ready());
        assert_eq!(ar.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }
}
