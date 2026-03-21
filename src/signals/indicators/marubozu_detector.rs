//! Marubozu Detector indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Marubozu Detector.
///
/// A marubozu is a strong candle with little to no wicks — the open is at (or near)
/// the low and the close is at (or near) the high (bullish), or vice-versa (bearish).
/// They signal strong, one-directional conviction.
///
/// Output encoding:
/// - `+1.0` — bullish marubozu (close near high, open near low)
/// - `-1.0` — bearish marubozu (close near low, open near high)
/// - `0.0`  — no marubozu detected
///
/// A bar qualifies when:
/// - The body (|close − open|) is at least `min_body_pct` of the bar's range.
/// - Both upper and lower wicks are at most `max_wick_pct` of the bar's range.
///
/// This indicator is always ready from the first bar (no warm-up required).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MarubozuDetector;
/// use fin_primitives::signals::Signal;
/// let md = MarubozuDetector::new("maru", 90, 5).unwrap();
/// assert!(md.is_ready());
/// ```
pub struct MarubozuDetector {
    name: String,
    min_body_pct: Decimal,
    max_wick_pct: Decimal,
}

impl MarubozuDetector {
    /// Constructs a new `MarubozuDetector`.
    ///
    /// # Parameters
    /// - `min_body_pct`: minimum body as a percentage of range (0–100). Typical: 90.
    /// - `max_wick_pct`: maximum wick as a percentage of range (0–100). Typical: 5.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if percentages are out of range or inconsistent.
    pub fn new(
        name: impl Into<String>,
        min_body_pct: u32,
        max_wick_pct: u32,
    ) -> Result<Self, FinError> {
        if min_body_pct > 100 || max_wick_pct > 100 {
            return Err(FinError::InvalidInput("percentage out of range".into()));
        }
        Ok(Self {
            name: name.into(),
            min_body_pct: Decimal::from(min_body_pct),
            max_wick_pct: Decimal::from(max_wick_pct),
        })
    }
}

impl Signal for MarubozuDetector {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let hundred = Decimal::from(100u32);
        let body = (bar.close - bar.open).abs();
        let upper_wick = bar.high - bar.close.max(bar.open);
        let lower_wick = bar.close.min(bar.open) - bar.low;

        let body_pct = body
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(hundred)
            .ok_or(FinError::ArithmeticOverflow)?;

        let upper_wick_pct = upper_wick
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(hundred)
            .ok_or(FinError::ArithmeticOverflow)?;

        let lower_wick_pct = lower_wick
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(hundred)
            .ok_or(FinError::ArithmeticOverflow)?;

        if body_pct >= self.min_body_pct
            && upper_wick_pct <= self.max_wick_pct
            && lower_wick_pct <= self.max_wick_pct
        {
            // Determine direction
            let direction = if bar.close > bar.open {
                Decimal::ONE
            } else {
                Decimal::NEGATIVE_ONE
            };
            Ok(SignalValue::Scalar(direction))
        } else {
            Ok(SignalValue::Scalar(Decimal::ZERO))
        }
    }

    fn is_ready(&self) -> bool {
        true // No warm-up required
    }

    fn period(&self) -> usize {
        1
    }

    fn reset(&mut self) {
        // Stateless — nothing to clear
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn make_bar(open: &str, high: &str, low: &str, close: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(open.parse().unwrap()).unwrap(),
            high: Price::new(high.parse().unwrap()).unwrap(),
            low: Price::new(low.parse().unwrap()).unwrap(),
            close: Price::new(close.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_invalid_pct_fails() {
        assert!(MarubozuDetector::new("m", 101, 5).is_err());
        assert!(MarubozuDetector::new("m", 90, 101).is_err());
    }

    #[test]
    fn test_always_ready() {
        let md = MarubozuDetector::new("m", 90, 5).unwrap();
        assert!(md.is_ready());
    }

    #[test]
    fn test_bullish_marubozu_detected() {
        let mut md = MarubozuDetector::new("m", 90, 5).unwrap();
        // Open at low, close at high — perfect bullish marubozu
        let v = md.update_bar(&make_bar("10", "20", "10", "20")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bearish_marubozu_detected() {
        let mut md = MarubozuDetector::new("m", 90, 5).unwrap();
        // Open at high, close at low — perfect bearish marubozu
        let v = md.update_bar(&make_bar("20", "20", "10", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_doji_not_marubozu() {
        let mut md = MarubozuDetector::new("m", 90, 5).unwrap();
        // Doji: equal open and close — body too small
        let v = md.update_bar(&make_bar("15", "20", "10", "15")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_normal_candle_not_marubozu() {
        let mut md = MarubozuDetector::new("m", 90, 5).unwrap();
        // Candle with significant upper wick — fails wick test
        let v = md.update_bar(&make_bar("10", "20", "10", "14")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_flat_bar_returns_zero() {
        let mut md = MarubozuDetector::new("m", 90, 5).unwrap();
        let v = md.update_bar(&make_bar("10", "10", "10", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset_has_no_effect() {
        let mut md = MarubozuDetector::new("m", 90, 5).unwrap();
        md.reset();
        assert!(md.is_ready());
    }

    #[test]
    fn test_loose_thresholds_detect_more() {
        let mut md_strict = MarubozuDetector::new("m", 90, 2).unwrap();
        let mut md_loose = MarubozuDetector::new("m", 60, 20).unwrap();
        // Bar with body = 75% of range, wicks within loose limits
        let b = make_bar("11", "18", "10", "17"); // body=6, range=8, upper_wick=1(12.5%), lower_wick=1(12.5%)
        let vs = md_strict.update_bar(&b).unwrap();
        let vl = md_loose.update_bar(&b).unwrap();
        assert_eq!(vs, SignalValue::Scalar(dec!(0)));   // strict misses it
        assert_eq!(vl, SignalValue::Scalar(dec!(1)));   // loose catches it
    }
}
