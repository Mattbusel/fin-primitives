//! Tail Ratio Percent — fraction of total wick length that is upper wick.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Tail Ratio Percent — `upper_wick / (upper_wick + lower_wick)`.
///
/// Measures the balance between upper and lower wicks on a single bar:
/// - **1.0**: all wick is above the body (bearish rejection / supply pressure).
/// - **0.0**: all wick is below the body (bullish rejection / demand pressure).
/// - **0.5**: equal upper and lower wicks (balanced pressure).
///
/// Returns [`SignalValue::Unavailable`] when total wick is zero (no wicks on the bar,
/// or a doji with no wicks beyond the body).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TailRatioPct;
/// use fin_primitives::signals::Signal;
/// let trp = TailRatioPct::new("tail_pct");
/// assert_eq!(trp.period(), 1);
/// ```
pub struct TailRatioPct {
    name: String,
}

impl TailRatioPct {
    /// Constructs a new `TailRatioPct`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for TailRatioPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let upper_wick = bar.upper_wick();
        let lower_wick = bar.lower_wick();
        let total_wick = upper_wick + lower_wick;

        if total_wick.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = upper_wick
            .checked_div(total_wick)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio.clamp(Decimal::ZERO, Decimal::ONE)))
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
    fn test_trp_all_upper_wick_gives_one() {
        let mut s = TailRatioPct::new("trp");
        // open=close=90, high=110, low=90 → upper_wick=20, lower_wick=0 → 1.0
        let v = s.update_bar(&bar("90", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_trp_all_lower_wick_gives_zero() {
        let mut s = TailRatioPct::new("trp");
        // open=close=110, high=110, low=90 → upper_wick=0, lower_wick=20 → 0.0
        let v = s.update_bar(&bar("110", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_trp_equal_wicks_gives_half() {
        let mut s = TailRatioPct::new("trp");
        // open=100, close=100, high=110, low=90 → upper=10, lower=10 → 0.5
        let v = s.update_bar(&bar("100", "110", "90", "100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(0.5)).abs() < dec!(0.0001), "expected 0.5, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_trp_no_wicks_unavailable() {
        let mut s = TailRatioPct::new("trp");
        // open=low=90, close=high=110 → no wicks → Unavailable
        let v = s.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_trp_output_in_unit_interval() {
        let mut s = TailRatioPct::new("trp");
        let bars = [
            bar("100","115","95","110"),
            bar("110","120","105","112"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = s.update_bar(b).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "out of [0,1]: {v}");
            }
        }
    }
}
