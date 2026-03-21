//! Harami candlestick pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Harami candlestick pattern detector.
///
/// A harami occurs when a small bar is completely contained within the body of the
/// preceding bar. It signals potential trend reversal or consolidation.
///
/// Output encoding:
/// - `+1.0` — bullish harami (preceded by a bearish bar)
/// - `-1.0` — bearish harami (preceded by a bullish bar)
/// - `0.0`  — no harami detected
///
/// The current bar's body must be at most `max_inner_pct` of the previous bar's
/// body size to qualify.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HaramiDetector;
/// use fin_primitives::signals::Signal;
/// let hd = HaramiDetector::new("harami", 50).unwrap();
/// assert!(!hd.is_ready());
/// ```
pub struct HaramiDetector {
    name: String,
    max_inner_pct: Decimal,
    prev: Option<BarInput>,
}

impl HaramiDetector {
    /// Constructs a new `HaramiDetector`.
    ///
    /// `max_inner_pct`: the current bar's body must be at most this percentage of the
    /// previous bar's body to qualify (0–100). Typical value: 50.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `max_inner_pct > 100`.
    pub fn new(name: impl Into<String>, max_inner_pct: u32) -> Result<Self, FinError> {
        if max_inner_pct > 100 {
            return Err(FinError::InvalidInput("max_inner_pct out of range".into()));
        }
        Ok(Self {
            name: name.into(),
            max_inner_pct: Decimal::from(max_inner_pct),
            prev: None,
        })
    }
}

impl Signal for HaramiDetector {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(ref prev) = self.prev {
            let prev_body_high = prev.close.max(prev.open);
            let prev_body_low = prev.close.min(prev.open);
            let prev_body = prev_body_high - prev_body_low;

            let cur_body_high = bar.close.max(bar.open);
            let cur_body_low = bar.close.min(bar.open);
            let cur_body = cur_body_high - cur_body_low;

            // Current body must fit inside previous body
            let contained = cur_body_high <= prev_body_high && cur_body_low >= prev_body_low;

            // Current body must be sufficiently smaller
            let small_enough = if prev_body.is_zero() {
                false
            } else {
                let pct = cur_body
                    .checked_div(prev_body)
                    .ok_or(FinError::ArithmeticOverflow)?
                    .checked_mul(Decimal::from(100u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                pct <= self.max_inner_pct
            };

            if contained && small_enough {
                // Bullish harami: preceded by bearish bar
                if prev.close < prev.open {
                    SignalValue::Scalar(Decimal::ONE)
                } else {
                    // Bearish harami: preceded by bullish bar
                    SignalValue::Scalar(Decimal::NEGATIVE_ONE)
                }
            } else {
                SignalValue::Scalar(Decimal::ZERO)
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
    fn test_invalid_pct_fails() {
        assert!(HaramiDetector::new("h", 101).is_err());
    }

    #[test]
    fn test_first_bar_unavailable() {
        let mut hd = HaramiDetector::new("h", 50).unwrap();
        assert_eq!(hd.update_bar(&bar("10", "12", "9", "8")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ready_after_first_bar() {
        let mut hd = HaramiDetector::new("h", 50).unwrap();
        hd.update_bar(&bar("10", "12", "9", "8")).unwrap();
        assert!(hd.is_ready());
    }

    #[test]
    fn test_bullish_harami() {
        let mut hd = HaramiDetector::new("h", 50).unwrap();
        // Large bearish bar: open=20, close=10 → body=10
        hd.update_bar(&bar("20", "21", "9", "10")).unwrap();
        // Small bullish bar inside: open=12, close=14, body=2 (20% of 10)
        let v = hd.update_bar(&bar("12", "14", "11", "14")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bearish_harami() {
        let mut hd = HaramiDetector::new("h", 50).unwrap();
        // Large bullish bar: open=10, close=20 → body=10
        hd.update_bar(&bar("10", "21", "9", "20")).unwrap();
        // Small bearish bar inside: open=18, close=16, body=2 (20% of 10)
        let v = hd.update_bar(&bar("18", "19", "15", "16")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_large_inner_bar_not_harami() {
        let mut hd = HaramiDetector::new("h", 50).unwrap();
        // Large bearish bar: body=10
        hd.update_bar(&bar("20", "21", "9", "10")).unwrap();
        // Inner bar body=7 (70% of 10) — too large for 50% threshold
        let v = hd.update_bar(&bar("18", "19", "12", "11")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut hd = HaramiDetector::new("h", 50).unwrap();
        hd.update_bar(&bar("10", "12", "9", "8")).unwrap();
        assert!(hd.is_ready());
        hd.reset();
        assert!(!hd.is_ready());
    }
}
