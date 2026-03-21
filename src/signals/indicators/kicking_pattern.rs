//! Kicking Pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Kicking Pattern candlestick detector.
///
/// One of the strongest reversal signals: two opposite marubozu candles separated
/// by a gap. The gap must be in the direction of the new trend.
///
/// Criteria:
/// - Bar 1 and Bar 2 are marubozu candles (body ≥ `min_body_pct` of range).
/// - Bar 2 gaps away from Bar 1: open of Bar 2 is beyond the close of Bar 1
///   (gap-up for bullish kicking, gap-down for bearish kicking).
///
/// Output:
/// - `+1.0` — Bullish kicking (bearish bar 1, bullish bar 2 with gap-up).
/// - `-1.0` — Bearish kicking (bullish bar 1, bearish bar 2 with gap-down).
/// - `0.0`  — No pattern.
///
/// Returns `SignalValue::Unavailable` on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::KickingPattern;
/// use fin_primitives::signals::Signal;
/// let kp = KickingPattern::new("kp", 80).unwrap();
/// assert_eq!(kp.period(), 2);
/// ```
pub struct KickingPattern {
    name: String,
    min_body_pct: Decimal,
    prev: Option<BarInput>,
}

impl KickingPattern {
    /// Constructs a new `KickingPattern` detector.
    ///
    /// `min_body_pct`: minimum body as % of range to qualify as marubozu (0–100). Typical: 80.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `min_body_pct > 100`.
    pub fn new(name: impl Into<String>, min_body_pct: u32) -> Result<Self, FinError> {
        if min_body_pct > 100 {
            return Err(FinError::InvalidInput("min_body_pct out of range".into()));
        }
        Ok(Self {
            name: name.into(),
            min_body_pct: Decimal::from(min_body_pct),
            prev: None,
        })
    }

    fn is_marubozu(&self, bar: &BarInput) -> Result<bool, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(false);
        }
        let body = (bar.close - bar.open).abs();
        let pct = body
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(pct >= self.min_body_pct)
    }
}

impl Signal for KickingPattern {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(ref prev) = self.prev {
            if !self.is_marubozu(prev)? || !self.is_marubozu(bar)? {
                SignalValue::Scalar(Decimal::ZERO)
            } else {
                let prev_bearish = prev.close < prev.open;
                let cur_bullish = bar.close > bar.open;
                // Bullish kicking: prev bearish marubozu, cur bullish marubozu gapping up
                if prev_bearish && cur_bullish && bar.open > prev.open {
                    SignalValue::Scalar(Decimal::ONE)
                // Bearish kicking: prev bullish marubozu, cur bearish marubozu gapping down
                } else if !prev_bearish && !cur_bullish && bar.open < prev.open {
                    SignalValue::Scalar(Decimal::NEGATIVE_ONE)
                } else {
                    SignalValue::Scalar(Decimal::ZERO)
                }
            }
        } else {
            SignalValue::Unavailable
        };

        self.prev = Some(*bar);
        Ok(result)
    }

    fn is_ready(&self) -> bool {
        self.prev.is_some()
    }

    fn period(&self) -> usize {
        2
    }

    fn reset(&mut self) {
        self.prev = None;
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
    fn test_invalid_pct() { assert!(KickingPattern::new("kp", 101).is_err()); }

    #[test]
    fn test_first_bar_unavailable() {
        let mut kp = KickingPattern::new("kp", 80).unwrap();
        assert_eq!(kp.update_bar(&bar("20", "20", "10", "10")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bullish_kicking() {
        let mut kp = KickingPattern::new("kp", 80).unwrap();
        // Bearish marubozu: open=20=high, close=10=low → body=10=range=100%
        kp.update_bar(&bar("20", "20", "10", "10")).unwrap();
        // Bullish marubozu: open=25=low > prev open=20, close=35=high → 100% body, gap up
        let v = kp.update_bar(&bar("25", "35", "25", "35")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bearish_kicking() {
        let mut kp = KickingPattern::new("kp", 80).unwrap();
        // Bullish marubozu
        kp.update_bar(&bar("10", "20", "10", "20")).unwrap();
        // Bearish marubozu: open=5 < prev open=10 → gap down
        let v = kp.update_bar(&bar("5", "5", "1", "1")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_no_pattern_no_gap() {
        let mut kp = KickingPattern::new("kp", 80).unwrap();
        kp.update_bar(&bar("20", "20", "10", "10")).unwrap();
        // Bullish but no gap (open <= prev open)
        let v = kp.update_bar(&bar("15", "25", "15", "25")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut kp = KickingPattern::new("kp", 80).unwrap();
        kp.update_bar(&bar("20", "20", "10", "10")).unwrap();
        kp.reset();
        assert!(!kp.is_ready());
    }
}
