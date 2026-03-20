//! Upper Wick Percent — upper wick as a percentage of the total bar range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Upper Wick Percent — `upper_wick / (high - low) * 100`.
///
/// Measures what fraction of the bar's range is occupied by the upper wick:
/// - **High value**: significant rejection from the high (bearish wick pressure).
/// - **0**: no upper wick (closed at or above open with no upper shadow, or close == high).
/// - **100**: entire bar is upper wick (e.g., a dragonfly doji).
///
/// Returns [`SignalValue::Unavailable`] when the bar range is zero (flat bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpperWickPct;
/// use fin_primitives::signals::Signal;
/// let uwp = UpperWickPct::new("uwp");
/// assert_eq!(uwp.period(), 1);
/// ```
pub struct UpperWickPct {
    name: String,
}

impl UpperWickPct {
    /// Constructs a new `UpperWickPct`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for UpperWickPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let upper_wick = bar.upper_wick();
        let pct = upper_wick
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct.clamp(Decimal::ZERO, Decimal::ONE_HUNDRED)))
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
    fn test_uwp_no_upper_wick() {
        let mut s = UpperWickPct::new("uwp");
        // close=high=110, open=90, low=90 → upper_wick=0
        let v = s.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_uwp_all_upper_wick() {
        let mut s = UpperWickPct::new("uwp");
        // open=close=90 (doji), high=110, low=90 → wick=20/20=100%
        let v = s.update_bar(&bar("90", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_uwp_half() {
        let mut s = UpperWickPct::new("uwp");
        // open=90, close=100, high=110, low=90 → upper_wick=10, range=20 → 50%
        let v = s.update_bar(&bar("90", "110", "90", "100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(50)).abs() < dec!(0.0001), "expected 50%, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_uwp_flat_bar_unavailable() {
        let mut s = UpperWickPct::new("uwp");
        assert_eq!(s.update_bar(&bar("100", "100", "100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_uwp_in_range() {
        let mut s = UpperWickPct::new("uwp");
        let bars = [
            bar("100", "115", "95", "110"),
            bar("110", "120", "105", "108"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = s.update_bar(b).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(100), "out of [0,100]: {v}");
            }
        }
    }
}
