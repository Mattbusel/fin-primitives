//! Body-to-Shadow Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Body-to-Shadow Ratio — the candle body size relative to total shadow length.
///
/// ```text
/// body         = |close - open|
/// total_shadow = (high - low) - body  = upper_shadow + lower_shadow
/// ratio        = body / total_shadow
/// ```
///
/// - **High value**: most of the bar's range is body (strong directional move).
/// - **Near zero**: small body with large shadows (indecision or rejection candle).
/// - Returns [`SignalValue::Unavailable`] when `total_shadow == 0`
///   (entire range is body, or bar has no range at all).
/// - Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyToShadowRatio;
/// use fin_primitives::signals::Signal;
///
/// let btsr = BodyToShadowRatio::new("btsr").unwrap();
/// assert_eq!(btsr.period(), 1);
/// ```
pub struct BodyToShadowRatio {
    name: String,
}

impl BodyToShadowRatio {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for BodyToShadowRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.body_size();
        let range = bar.range();
        let total_shadow = range - body;

        if total_shadow.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = body
            .checked_div(total_shadow)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio))
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
    fn test_btsr_always_ready() {
        let btsr = BodyToShadowRatio::new("btsr").unwrap();
        assert!(btsr.is_ready());
    }

    #[test]
    fn test_btsr_no_shadows_unavailable() {
        // body = range → total_shadow = 0 → Unavailable
        let mut btsr = BodyToShadowRatio::new("btsr").unwrap();
        let result = btsr.update_bar(&bar("100", "110", "100", "110")).unwrap();
        assert_eq!(result, SignalValue::Unavailable);
    }

    #[test]
    fn test_btsr_equal_body_and_shadows() {
        // open=100, close=105, high=110, low=95
        // body=5, range=15, total_shadow=10, ratio=0.5
        let mut btsr = BodyToShadowRatio::new("btsr").unwrap();
        let result = btsr.update_bar(&bar("100", "110", "95", "105")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_btsr_doji_small_body() {
        // open=100, close=101, high=110, low=90 → body=1, range=20, shadow=19
        let mut btsr = BodyToShadowRatio::new("btsr").unwrap();
        if let SignalValue::Scalar(v) = btsr.update_bar(&bar("100", "110", "90", "101")).unwrap() {
            assert!(v < dec!(0.1), "doji-like bar should have low ratio: {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
