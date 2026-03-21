//! Directional Candle Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Directional Candle Ratio.
///
/// Tracks the ratio of bullish candles to total candles in a rolling window.
/// A bullish candle is one where `close > open`.
///
/// Formula: `dcr = bullish_count / period`
///
/// - 1.0: all candles in the window are bullish.
/// - 0.0: all candles are bearish or doji.
/// - 0.5: balanced (or all dojis).
///
/// Doji bars (`close == open`) are counted as neither bullish nor bearish
/// (not included in bullish count).
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DirectionalCandleRatio;
/// use fin_primitives::signals::Signal;
/// let dcr = DirectionalCandleRatio::new("dcr_20", 20).unwrap();
/// assert_eq!(dcr.period(), 20);
/// ```
pub struct DirectionalCandleRatio {
    name: String,
    period: usize,
    /// +1 bullish, 0 doji/bearish stored per bar
    directions: VecDeque<i8>,
}

impl DirectionalCandleRatio {
    /// Constructs a new `DirectionalCandleRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, directions: VecDeque::with_capacity(period) })
    }
}

impl Signal for DirectionalCandleRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let dir: i8 = if bar.close > bar.open { 1 } else { 0 };
        self.directions.push_back(dir);
        if self.directions.len() > self.period {
            self.directions.pop_front();
        }
        if self.directions.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let bull_count: i64 = self.directions.iter().map(|&d| i64::from(d)).sum();
        #[allow(clippy::cast_possible_truncation)]
        let ratio = Decimal::from(bull_count)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool {
        self.directions.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.directions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let hi = if op > cl { op } else { cl };
        let lo = if op < cl { op } else { cl };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(DirectionalCandleRatio::new("dcr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut dcr = DirectionalCandleRatio::new("dcr", 3).unwrap();
        assert_eq!(dcr.update_bar(&bar("10", "12")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_all_bullish_gives_one() {
        let mut dcr = DirectionalCandleRatio::new("dcr", 3).unwrap();
        for _ in 0..3 {
            dcr.update_bar(&bar("10", "12")).unwrap();
        }
        let v = dcr.update_bar(&bar("10", "12")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_all_bearish_gives_zero() {
        let mut dcr = DirectionalCandleRatio::new("dcr", 3).unwrap();
        for _ in 0..3 {
            dcr.update_bar(&bar("12", "10")).unwrap();
        }
        let v = dcr.update_bar(&bar("12", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_half_bullish() {
        let mut dcr = DirectionalCandleRatio::new("dcr", 4).unwrap();
        dcr.update_bar(&bar("10", "12")).unwrap();
        dcr.update_bar(&bar("12", "10")).unwrap();
        dcr.update_bar(&bar("10", "12")).unwrap();
        let v = dcr.update_bar(&bar("12", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_reset() {
        let mut dcr = DirectionalCandleRatio::new("dcr", 2).unwrap();
        dcr.update_bar(&bar("10", "12")).unwrap();
        dcr.update_bar(&bar("10", "12")).unwrap();
        assert!(dcr.is_ready());
        dcr.reset();
        assert!(!dcr.is_ready());
    }
}
