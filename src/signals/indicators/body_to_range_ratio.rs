//! Body-to-Range Ratio — candle body size as a fraction of the total range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Body-to-Range Ratio — `|close - open| / (high - low)`.
///
/// Measures how much of the bar's range is covered by the candle body:
/// - **1.0**: a Marubozu (body fills the entire range, no wicks).
/// - **0.0**: a pure Doji (no body, all wick).
/// - Values between 0 and 1 indicate partial wick formation.
///
/// Returns [`SignalValue::Unavailable`] when the bar range is zero (flat bar).
/// This is a period-1 indicator — it always emits on the second bar (period=1
/// since every bar is independent).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyToRangeRatio;
/// use fin_primitives::signals::Signal;
/// let btr = BodyToRangeRatio::new("btr");
/// assert_eq!(btr.period(), 1);
/// ```
pub struct BodyToRangeRatio {
    name: String,
}

impl BodyToRangeRatio {
    /// Constructs a new `BodyToRangeRatio`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for BodyToRangeRatio {
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
        let body = (bar.close - bar.open).abs();
        let ratio = body.checked_div(range).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio.min(Decimal::ONE)))
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
    fn test_btr_marubozu_gives_one() {
        let mut btr = BodyToRangeRatio::new("btr");
        // open=90, high=110, low=90, close=110 → body=20, range=20 → 1.0
        let v = btr.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_btr_doji_gives_zero() {
        let mut btr = BodyToRangeRatio::new("btr");
        // open=close=100, high=110, low=90 → body=0, range=20 → 0.0
        let v = btr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_btr_flat_bar_unavailable() {
        let mut btr = BodyToRangeRatio::new("btr");
        let v = btr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_btr_half_body() {
        let mut btr = BodyToRangeRatio::new("btr");
        // open=95, high=110, low=90, close=105 → body=10, range=20 → 0.5
        let v = btr.update_bar(&bar("95", "110", "90", "105")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(0.5)).abs() < dec!(0.0001), "expected 0.5, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_btr_always_ready() {
        let btr = BodyToRangeRatio::new("btr");
        assert!(btr.is_ready());
    }

    #[test]
    fn test_btr_output_in_unit_interval() {
        let mut btr = BodyToRangeRatio::new("btr");
        let bars = [
            bar("100", "110", "90", "105"),
            bar("105", "115", "100", "102"),
            bar("102", "108", "98", "100"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = btr.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "ratio must be >= 0: {v}");
                assert!(v <= dec!(1), "ratio must be <= 1: {v}");
            }
        }
    }

    #[test]
    fn test_btr_period_and_name() {
        let btr = BodyToRangeRatio::new("my_btr");
        assert_eq!(btr.period(), 1);
        assert_eq!(btr.name(), "my_btr");
    }
}
