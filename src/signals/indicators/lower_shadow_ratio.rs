//! Lower Shadow Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Lower Shadow Ratio — the fraction of the bar's range occupied by the lower shadow.
///
/// ```text
/// lower_shadow = min(open, close) - low
/// ratio        = lower_shadow / (high - low)
/// ```
///
/// Returns `0` when the bar has no range (`high == low`).
/// Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LowerShadowRatio;
/// use fin_primitives::signals::Signal;
///
/// let lsr = LowerShadowRatio::new("lsr").unwrap();
/// assert_eq!(lsr.period(), 1);
/// ```
pub struct LowerShadowRatio {
    name: String,
}

impl LowerShadowRatio {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for LowerShadowRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let body_lo = bar.body_low();
        let lower_shadow = body_lo - bar.low;
        Ok(SignalValue::Scalar(lower_shadow / range))
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
    fn test_lsr_always_ready() {
        let lsr = LowerShadowRatio::new("lsr").unwrap();
        assert!(lsr.is_ready());
    }

    #[test]
    fn test_lsr_no_range() {
        let mut lsr = LowerShadowRatio::new("lsr").unwrap();
        let result = lsr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_lsr_all_lower_shadow() {
        // open=100, close=100, high=100, low=90 → lower_shadow=10, range=10 → ratio=1
        let mut lsr = LowerShadowRatio::new("lsr").unwrap();
        let result = lsr.update_bar(&bar("100", "100", "90", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_lsr_no_lower_shadow() {
        // bearish bar: open=110, close=100, high=110, low=100 → lower_shadow=0 → ratio=0
        let mut lsr = LowerShadowRatio::new("lsr").unwrap();
        let result = lsr.update_bar(&bar("110", "110", "100", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_lsr_partial_lower_shadow() {
        // open=105, close=110, high=110, low=100 → lower_shadow=5, range=10 → ratio=0.5
        let mut lsr = LowerShadowRatio::new("lsr").unwrap();
        let result = lsr.update_bar(&bar("105", "110", "100", "110")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.5)));
    }
}
