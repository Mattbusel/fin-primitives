//! Close-to-High Ratio — how far the close is from the bar high, relative to the range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close-to-High Ratio — `(high - close) / (high - low)`.
///
/// Measures where the close sits within the bar range, viewed from the high:
/// - **0.0**: close equals the high (maximum bullish close — closed at top).
/// - **1.0**: close equals the low (maximum bearish close — closed at bottom).
/// - **0.5**: close at the midpoint.
///
/// Complementary to [`crate::signals::indicators::RollingHighLowPosition`] which uses
/// a rolling window; this operates on a single bar.
///
/// Returns [`SignalValue::Unavailable`] when the bar range is zero (flat bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseToHighRatio;
/// use fin_primitives::signals::Signal;
/// let cthr = CloseToHighRatio::new("cthr");
/// assert_eq!(cthr.period(), 1);
/// ```
pub struct CloseToHighRatio {
    name: String,
}

impl CloseToHighRatio {
    /// Constructs a new `CloseToHighRatio`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for CloseToHighRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let ratio = (bar.high - bar.close)
            .checked_div(range)
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
    fn test_cthr_close_at_high_gives_zero() {
        let mut cthr = CloseToHighRatio::new("cthr");
        // close=high → (high-close)/range = 0
        let v = cthr.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cthr_close_at_low_gives_one() {
        let mut cthr = CloseToHighRatio::new("cthr");
        // close=low → (high-close)/range = 1
        let v = cthr.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cthr_close_at_midpoint() {
        let mut cthr = CloseToHighRatio::new("cthr");
        // close=100, high=110, low=90 → (110-100)/20 = 0.5
        let v = cthr.update_bar(&bar("110", "90", "100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(0.5)).abs() < dec!(0.0001), "expected 0.5, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cthr_flat_bar_unavailable() {
        let mut cthr = CloseToHighRatio::new("cthr");
        assert_eq!(cthr.update_bar(&bar("100", "100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cthr_output_in_unit_interval() {
        let mut cthr = CloseToHighRatio::new("cthr");
        let bars = [
            bar("110", "90", "105"),
            bar("108", "92", "98"),
            bar("115", "85", "100"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = cthr.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "ratio must be >= 0: {v}");
                assert!(v <= dec!(1), "ratio must be <= 1: {v}");
            }
        }
    }

    #[test]
    fn test_cthr_period_and_name() {
        let cthr = CloseToHighRatio::new("my_cthr");
        assert_eq!(cthr.period(), 1);
        assert_eq!(cthr.name(), "my_cthr");
    }
}
