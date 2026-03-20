//! Shadow Imbalance indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Shadow Imbalance — the directional difference between upper and lower
/// shadows, normalized by the total range.
///
/// ```text
/// upper_shadow = high - max(open, close)
/// lower_shadow = min(open, close) - low
/// imbalance    = (upper_shadow - lower_shadow) / (high - low)
/// ```
///
/// - `+1` → all range is upper shadow (bearish wick dominance)  
/// - `-1` → all range is lower shadow (bullish wick — hammer shape)  
/// - `0` → balanced wicks  
///
/// Returns `0` when the bar has no range. Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ShadowImbalance;
/// use fin_primitives::signals::Signal;
///
/// let si = ShadowImbalance::new("si").unwrap();
/// assert_eq!(si.period(), 1);
/// ```
pub struct ShadowImbalance {
    name: String,
}

impl ShadowImbalance {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for ShadowImbalance {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let body_hi = bar.close.max(bar.open);
        let body_lo = bar.close.min(bar.open);
        let upper_shadow = bar.high - body_hi;
        let lower_shadow = body_lo - bar.low;
        Ok(SignalValue::Scalar((upper_shadow - lower_shadow) / range))
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
    fn test_si_no_range() {
        let mut si = ShadowImbalance::new("si").unwrap();
        assert_eq!(si.update_bar(&bar("100", "100", "100", "100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_si_all_upper_shadow() {
        // open=100, close=100, high=110, low=100 → upper=10, lower=0, range=10 → +1
        let mut si = ShadowImbalance::new("si").unwrap();
        assert_eq!(si.update_bar(&bar("100", "110", "100", "100")).unwrap(), SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_si_all_lower_shadow() {
        // open=100, close=100, high=100, low=90 → upper=0, lower=10, range=10 → -1
        let mut si = ShadowImbalance::new("si").unwrap();
        assert_eq!(si.update_bar(&bar("100", "100", "90", "100")).unwrap(), SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_si_balanced_shadows() {
        // open=100, close=100, high=110, low=90 → upper=10, lower=10, range=20 → 0
        let mut si = ShadowImbalance::new("si").unwrap();
        assert_eq!(si.update_bar(&bar("100", "110", "90", "100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_si_body_consumes_range() {
        // fully bullish: open=90, close=110, high=110, low=90 → upper=0, lower=0 → 0
        let mut si = ShadowImbalance::new("si").unwrap();
        assert_eq!(si.update_bar(&bar("90", "110", "90", "110")).unwrap(), SignalValue::Scalar(dec!(0)));
    }
}
