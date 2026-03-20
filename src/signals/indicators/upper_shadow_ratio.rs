//! Upper Shadow Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Upper Shadow Ratio — the fraction of the bar's range occupied by the upper shadow.
///
/// ```text
/// upper_shadow = high - max(open, close)
/// ratio        = upper_shadow / (high - low)
/// ```
///
/// Returns `0` when the bar has no range (`high == low`).
/// Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpperShadowRatio;
/// use fin_primitives::signals::Signal;
///
/// let usr = UpperShadowRatio::new("usr").unwrap();
/// assert_eq!(usr.period(), 1);
/// ```
pub struct UpperShadowRatio {
    name: String,
}

impl UpperShadowRatio {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for UpperShadowRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let body_hi = bar.close.max(bar.open);
        let upper_shadow = bar.high - body_hi;
        Ok(SignalValue::Scalar(upper_shadow / range))
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
    fn test_usr_always_ready() {
        let usr = UpperShadowRatio::new("usr").unwrap();
        assert!(usr.is_ready());
    }

    #[test]
    fn test_usr_no_range() {
        let mut usr = UpperShadowRatio::new("usr").unwrap();
        let result = usr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_usr_all_upper_shadow() {
        // open=100, close=100, high=110, low=100 → upper_shadow=10, range=10 → ratio=1
        let mut usr = UpperShadowRatio::new("usr").unwrap();
        let result = usr.update_bar(&bar("100", "110", "100", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_usr_no_upper_shadow() {
        // bullish bar: open=100, close=110, high=110, low=100 → upper_shadow=0 → ratio=0
        let mut usr = UpperShadowRatio::new("usr").unwrap();
        let result = usr.update_bar(&bar("100", "110", "100", "110")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_usr_partial_upper_shadow() {
        // open=100, close=105, high=110, low=100 → upper_shadow=5, range=10 → ratio=0.5
        let mut usr = UpperShadowRatio::new("usr").unwrap();
        let result = usr.update_bar(&bar("100", "110", "100", "105")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.5)));
    }
}
