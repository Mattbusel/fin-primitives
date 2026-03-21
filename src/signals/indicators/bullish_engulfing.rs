//! Bullish Engulfing pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Bullish Engulfing Candlestick Pattern.
///
/// A classic two-bar reversal pattern that signals a potential bullish reversal:
/// - Bar 1: bearish candle (close < open).
/// - Bar 2: bullish candle (close > open) whose body **fully engulfs** bar 1's body.
///   Specifically: `bar2.open <= bar1.close AND bar2.close >= bar1.open`.
///
/// Output:
/// - `+1.0`: bullish engulfing pattern detected.
/// - `0.0`: no pattern.
///
/// Returns `SignalValue::Unavailable` on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BullishEngulfing;
/// use fin_primitives::signals::Signal;
/// let bull = BullishEngulfing::new("bull_eng");
/// assert_eq!(bull.period(), 2);
/// ```
pub struct BullishEngulfing {
    name: String,
    prev: Option<BarInput>,
}

impl BullishEngulfing {
    /// Constructs a new `BullishEngulfing`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev: None }
    }
}

impl Signal for BullishEngulfing {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(ref prev) = self.prev {
            let prev_bearish = prev.close < prev.open;
            let cur_bullish = bar.close > bar.open;
            // Engulfing: current open <= prev close, current close >= prev open
            let engulfs = bar.open <= prev.close && bar.close >= prev.open;

            if prev_bearish && cur_bullish && engulfs {
                SignalValue::Scalar(Decimal::ONE)
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
    fn test_first_bar_unavailable() {
        let mut bull = BullishEngulfing::new("bull");
        assert_eq!(bull.update_bar(&bar("10", "12", "9", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bullish_engulfing_detected() {
        let mut bull = BullishEngulfing::new("bull");
        // Bar 1: bearish, open=12, close=10
        bull.update_bar(&bar("12", "13", "9", "10")).unwrap();
        // Bar 2: bullish engulfing, open=9 <= prev close=10, close=13 >= prev open=12
        let v = bull.update_bar(&bar("9", "14", "8", "13")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_no_pattern_when_not_bearish_first() {
        let mut bull = BullishEngulfing::new("bull");
        // Bar 1: bullish
        bull.update_bar(&bar("10", "13", "9", "12")).unwrap();
        // Bar 2: even with engulf criteria, no pattern since bar1 is bullish
        let v = bull.update_bar(&bar("9", "14", "8", "13")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_no_pattern_without_full_engulf() {
        let mut bull = BullishEngulfing::new("bull");
        // Bar 1: bearish, open=12, close=10
        bull.update_bar(&bar("12", "13", "9", "10")).unwrap();
        // Bar 2: bullish but open=11 > prev close=10 → doesn't engulf from bottom
        let v = bull.update_bar(&bar("11", "14", "10", "13")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut bull = BullishEngulfing::new("bull");
        bull.update_bar(&bar("12", "13", "9", "10")).unwrap();
        assert!(bull.is_ready());
        bull.reset();
        assert!(!bull.is_ready());
    }
}
