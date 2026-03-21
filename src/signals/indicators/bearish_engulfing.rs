//! Bearish Engulfing pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Bearish Engulfing Candlestick Pattern.
///
/// A classic two-bar reversal pattern that signals a potential bearish reversal:
/// - Bar 1: bullish candle (close > open).
/// - Bar 2: bearish candle (close < open) whose body **fully engulfs** bar 1's body.
///   Specifically: `bar2.open >= bar1.close AND bar2.close <= bar1.open`.
///
/// Output:
/// - `+1.0`: bearish engulfing pattern detected.
/// - `0.0`: no pattern.
///
/// Returns `SignalValue::Unavailable` on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BearishEngulfing;
/// use fin_primitives::signals::Signal;
/// let be = BearishEngulfing::new("be");
/// assert_eq!(be.period(), 2);
/// ```
pub struct BearishEngulfing {
    name: String,
    prev: Option<BarInput>,
}

impl BearishEngulfing {
    /// Constructs a new `BearishEngulfing`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev: None }
    }
}

impl Signal for BearishEngulfing {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(ref prev) = self.prev {
            let prev_bullish = prev.close > prev.open;
            let cur_bearish = bar.close < bar.open;
            // Engulfing: current open >= prev close, current close <= prev open
            let engulfs = bar.open >= prev.close && bar.close <= prev.open;

            if prev_bullish && cur_bearish && engulfs {
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
        let mut be = BearishEngulfing::new("be");
        assert_eq!(be.update_bar(&bar("10", "12", "9", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bearish_engulfing_detected() {
        let mut be = BearishEngulfing::new("be");
        // Bar 1: bullish, open=10, close=12
        be.update_bar(&bar("10", "13", "9", "12")).unwrap();
        // Bar 2: bearish engulfing, open=13 >= prev close=12, close=9 <= prev open=10
        let v = be.update_bar(&bar("13", "14", "8", "9")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_no_pattern_when_not_bullish_first() {
        let mut be = BearishEngulfing::new("be");
        // Bar 1: bearish
        be.update_bar(&bar("12", "13", "9", "10")).unwrap();
        // Bar 2: would engulf but bar 1 is bearish
        let v = be.update_bar(&bar("13", "14", "8", "9")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_no_pattern_without_full_engulf() {
        let mut be = BearishEngulfing::new("be");
        // Bar 1: bullish, open=10, close=12
        be.update_bar(&bar("10", "13", "9", "12")).unwrap();
        // Bar 2: bearish but doesn't fully engulf (open=11 < prev close=12)
        let v = be.update_bar(&bar("11", "12", "8", "9")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut be = BearishEngulfing::new("be");
        be.update_bar(&bar("10", "13", "9", "12")).unwrap();
        assert!(be.is_ready());
        be.reset();
        assert!(!be.is_ready());
    }
}
