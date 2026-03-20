//! Open-to-High Ratio indicator -- how far the high extends from the open.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Open-to-High Ratio -- the upper extension of the bar from the open,
/// expressed as a percentage of the bar's total range.
///
/// ```text
/// ohr[t] = (high - open) / (high - low) * 100
/// ```
///
/// - 100% → high was far above open (strong upper breakout from open)
/// - 0%   → open was at the high (no upper extension from open)
///
/// Returns [`SignalValue::Unavailable`] if `high == low` (zero-range bar).
/// Ready after the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenHighRatio;
/// use fin_primitives::signals::Signal;
/// let ohr = OpenHighRatio::new("ohr");
/// assert_eq!(ohr.period(), 1);
/// ```
pub struct OpenHighRatio {
    name: String,
    ready: bool,
}

impl OpenHighRatio {
    /// Constructs a new `OpenHighRatio`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ready: false }
    }
}

impl Signal for OpenHighRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.ready }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.ready = true;
        let range = bar.high - bar.low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar((bar.high - bar.open) / range * Decimal::ONE_HUNDRED))
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

    fn bar(o: &str, h: &str, l: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: op,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ohr_open_at_low_is_100() {
        // open=90 (at the low), high=110, low=90 -> (110-90)/(110-90)*100 = 100
        let mut ohr = OpenHighRatio::new("ohr");
        let v = ohr.update_bar(&bar("90", "110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_ohr_open_at_high_is_0() {
        // open=110 (at the high), high=110, low=90 -> (110-110)/(110-90)*100 = 0
        let mut ohr = OpenHighRatio::new("ohr");
        let v = ohr.update_bar(&bar("110", "110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ohr_open_at_midpoint_is_50() {
        // open=100, high=110, low=90 -> (110-100)/(110-90)*100 = 50
        let mut ohr = OpenHighRatio::new("ohr");
        let v = ohr.update_bar(&bar("100", "110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_ohr_zero_range_unavailable() {
        let mut ohr = OpenHighRatio::new("ohr");
        let v = ohr.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_ohr_ready_after_first_bar() {
        let mut ohr = OpenHighRatio::new("ohr");
        assert!(!ohr.is_ready());
        ohr.update_bar(&bar("100", "110", "90")).unwrap();
        assert!(ohr.is_ready());
    }

    #[test]
    fn test_ohr_reset() {
        let mut ohr = OpenHighRatio::new("ohr");
        ohr.update_bar(&bar("100", "110", "90")).unwrap();
        ohr.reset();
        assert!(!ohr.is_ready());
    }
}
