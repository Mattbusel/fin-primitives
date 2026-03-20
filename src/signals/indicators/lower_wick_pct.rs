//! Lower Wick Percent — lower wick as a percentage of the total bar range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Lower Wick Percent — `lower_wick / (high - low) * 100`.
///
/// Measures what fraction of the bar's range is occupied by the lower wick:
/// - **High value**: significant rejection from the low (bullish wick pressure).
/// - **0**: no lower wick (closed at or below open with no lower shadow, or close == low).
/// - **100**: entire bar is lower wick (e.g., a gravestone doji).
///
/// Returns [`SignalValue::Unavailable`] when the bar range is zero (flat bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LowerWickPct;
/// use fin_primitives::signals::Signal;
/// let lwp = LowerWickPct::new("lwp");
/// assert_eq!(lwp.period(), 1);
/// ```
pub struct LowerWickPct {
    name: String,
}

impl LowerWickPct {
    /// Constructs a new `LowerWickPct`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for LowerWickPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let lower_wick = bar.lower_wick();
        let pct = lower_wick
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
    fn test_lwp_no_lower_wick() {
        let mut s = LowerWickPct::new("lwp");
        // open=low=90, close=high=110 → lower_wick=0
        let v = s.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_lwp_all_lower_wick() {
        let mut s = LowerWickPct::new("lwp");
        // open=close=110 (doji), high=110, low=90 → wick=20/20=100%
        let v = s.update_bar(&bar("110", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_lwp_half() {
        let mut s = LowerWickPct::new("lwp");
        // open=100, close=110, high=110, low=90 → lower_wick=10, range=20 → 50%
        let v = s.update_bar(&bar("100", "110", "90", "110")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(50)).abs() < dec!(0.0001), "expected 50%, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_lwp_flat_bar_unavailable() {
        let mut s = LowerWickPct::new("lwp");
        assert_eq!(s.update_bar(&bar("100", "100", "100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_lwp_in_range() {
        let mut s = LowerWickPct::new("lwp");
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
