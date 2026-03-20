//! Candle Efficiency indicator — body-to-range ratio.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Candle Efficiency — measures how efficiently price moved within each bar.
///
/// Defined as `|close - open| / (high - low)`, clipped to `[0, 1]`.
///
/// - **1.0**: the entire range was directional (pure body, no wicks).
/// - **0.0**: close == open (doji) or high == low (no range).
///
/// Returns [`SignalValue::Unavailable`] when `high == low` (zero range), avoiding
/// division by zero. Returns `Scalar(0)` for a doji (close == open, high != low).
///
/// This is a **period-1 indicator**: it produces a value on every bar with non-zero range.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CandleEfficiency;
/// use fin_primitives::signals::Signal;
/// let ce = CandleEfficiency::new("ce");
/// assert_eq!(ce.period(), 1);
/// ```
pub struct CandleEfficiency {
    name: String,
    ready: bool,
}

impl CandleEfficiency {
    /// Constructs a new `CandleEfficiency`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ready: false }
    }
}

impl Signal for CandleEfficiency {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let body = (bar.close - bar.open).abs();
        let efficiency = body
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?;
        self.ready = true;
        Ok(SignalValue::Scalar(efficiency.min(Decimal::ONE)))
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

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ce_full_body_is_one() {
        let mut ce = CandleEfficiency::new("ce");
        // open=100, high=110, low=100, close=110 → body=10, range=10, ratio=1
        let v = ce.update_bar(&bar("100", "110", "100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ce_doji_is_zero() {
        let mut ce = CandleEfficiency::new("ce");
        // open=close=105, but range exists
        let v = ce.update_bar(&bar("105", "110", "100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ce_half_body() {
        let mut ce = CandleEfficiency::new("ce");
        // open=100, high=110, low=100, close=105 → body=5, range=10, ratio=0.5
        let v = ce.update_bar(&bar("100", "110", "100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_ce_zero_range_unavailable() {
        let mut ce = CandleEfficiency::new("ce");
        let v = ce.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_ce_period_is_one() {
        let ce = CandleEfficiency::new("ce");
        assert_eq!(ce.period(), 1);
    }

    #[test]
    fn test_ce_is_ready_after_first_valid_bar() {
        let mut ce = CandleEfficiency::new("ce");
        assert!(!ce.is_ready());
        ce.update_bar(&bar("100", "110", "95", "108")).unwrap();
        assert!(ce.is_ready());
    }

    #[test]
    fn test_ce_reset() {
        let mut ce = CandleEfficiency::new("ce");
        ce.update_bar(&bar("100", "110", "95", "108")).unwrap();
        assert!(ce.is_ready());
        ce.reset();
        assert!(!ce.is_ready());
    }

    #[test]
    fn test_ce_output_in_unit_interval() {
        let mut ce = CandleEfficiency::new("ce");
        let bars = [
            bar("100", "110", "95", "108"),
            bar("108", "115", "105", "106"),
            bar("106", "108", "103", "107"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = ce.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "efficiency < 0: {v}");
                assert!(v <= dec!(1), "efficiency > 1: {v}");
            }
        }
    }
}
