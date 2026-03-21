//! Bar Quality Score indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Bar Quality Score.
///
/// A composite [0, 1] score measuring how "clean" or "decisive" a bar is.
/// Higher scores indicate strong directional bars with large bodies and minimal wicks.
/// Lower scores indicate indecision (doji, spinning top) or lack of range.
///
/// Formula:
/// - `body_ratio = |close − open| / (high − low)`  (body fraction of range)
/// - `direction_wick_penalty = (dominant_wick / range)` where dominant_wick is
///   the wick opposing the bar's direction (upper wick for bearish, lower for bullish)
/// - `score = body_ratio × (1 − direction_wick_penalty)`
///
/// Returns `SignalValue::Scalar(0.0)` for flat bars (zero range).
/// This indicator is always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarQualityScore;
/// use fin_primitives::signals::Signal;
/// let bqs = BarQualityScore::new("bqs").unwrap();
/// assert!(bqs.is_ready());
/// ```
pub struct BarQualityScore {
    name: String,
}

impl BarQualityScore {
    /// Constructs a new `BarQualityScore`.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for BarQualityScore {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let body = (bar.close - bar.open).abs();
        let body_ratio = body.checked_div(range).ok_or(FinError::ArithmeticOverflow)?;

        // Dominant wick opposes the direction
        let dominant_wick = if bar.close >= bar.open {
            // Bullish: lower wick opposes
            bar.close.min(bar.open) - bar.low
        } else {
            // Bearish: upper wick opposes
            bar.high - bar.close.max(bar.open)
        };

        let wick_penalty = dominant_wick
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?;
        let one_minus_penalty = Decimal::ONE
            .checked_sub(wick_penalty)
            .ok_or(FinError::ArithmeticOverflow)?;
        let score = body_ratio
            .checked_mul(one_minus_penalty)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(score.max(Decimal::ZERO)))
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn period(&self) -> usize {
        1
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
    fn test_always_ready() {
        let bqs = BarQualityScore::new("bqs").unwrap();
        assert!(bqs.is_ready());
    }

    #[test]
    fn test_perfect_marubozu_score_one() {
        let mut bqs = BarQualityScore::new("bqs").unwrap();
        // Perfect bullish marubozu: open=low, close=high, no wicks
        let v = bqs.update_bar(&bar("10", "20", "10", "20")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_doji_low_score() {
        let mut bqs = BarQualityScore::new("bqs").unwrap();
        // Doji: open==close, body=0 → score=0
        let v = bqs.update_bar(&bar("15", "20", "10", "15")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_flat_bar_zero() {
        let mut bqs = BarQualityScore::new("bqs").unwrap();
        let v = bqs.update_bar(&bar("10", "10", "10", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_score_in_range() {
        let mut bqs = BarQualityScore::new("bqs").unwrap();
        let v = bqs.update_bar(&bar("12", "18", "10", "16")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s >= dec!(0) && s <= dec!(1), "score out of [0,1]: {}", s);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_strong_bar_higher_than_weak() {
        let mut bqs = BarQualityScore::new("bqs").unwrap();
        // Strong: body=8, no opposing wick
        let strong = match bqs.update_bar(&bar("10", "20", "10", "18")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        // Weak: small body with large opposing wick
        let weak = match bqs.update_bar(&bar("15", "20", "10", "13")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        assert!(strong > weak);
    }
}
