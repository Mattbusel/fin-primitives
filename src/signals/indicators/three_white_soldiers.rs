//! Three White Soldiers candlestick pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Three White Soldiers pattern detector.
///
/// Three consecutive strong bullish candles, each opening within the previous
/// bar's body and closing near the high. Signals strong upward momentum and
/// continuation of a bullish trend.
///
/// Criteria for each of the three bars:
/// - Bullish (close > open).
/// - Body ≥ `min_body_pct` of range.
/// - Close ≥ `close_near_high_pct` of range from the low.
///
/// Output:
/// - `1.0` — Three White Soldiers detected.
/// - `0.0` — No pattern.
///
/// Returns `SignalValue::Unavailable` until 3 bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ThreeWhiteSoldiers;
/// use fin_primitives::signals::Signal;
/// let tws = ThreeWhiteSoldiers::new("tws", 60, 70).unwrap();
/// assert_eq!(tws.period(), 3);
/// ```
pub struct ThreeWhiteSoldiers {
    name: String,
    min_body_pct: Decimal,
    close_near_high_pct: Decimal,
    history: VecDeque<BarInput>,
}

impl ThreeWhiteSoldiers {
    /// Constructs a new `ThreeWhiteSoldiers` detector.
    ///
    /// - `min_body_pct`: minimum body as % of range. Typical: 60.
    /// - `close_near_high_pct`: minimum close position as % from low. Typical: 70.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if percentages are out of range.
    pub fn new(
        name: impl Into<String>,
        min_body_pct: u32,
        close_near_high_pct: u32,
    ) -> Result<Self, FinError> {
        if min_body_pct > 100 || close_near_high_pct > 100 {
            return Err(FinError::InvalidInput("percentage out of range".into()));
        }
        Ok(Self {
            name: name.into(),
            min_body_pct: Decimal::from(min_body_pct),
            close_near_high_pct: Decimal::from(close_near_high_pct),
            history: VecDeque::with_capacity(3),
        })
    }

    fn is_strong_bullish(&self, bar: &BarInput) -> Result<bool, FinError> {
        if bar.close <= bar.open {
            return Ok(false);
        }
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(false);
        }
        let hundred = Decimal::from(100u32);
        let body = bar.close - bar.open;
        let body_pct = body
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(hundred)
            .ok_or(FinError::ArithmeticOverflow)?;
        let close_from_low = bar.close - bar.low;
        let close_pct = close_from_low
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(hundred)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(body_pct >= self.min_body_pct && close_pct >= self.close_near_high_pct)
    }
}

impl Signal for ThreeWhiteSoldiers {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(*bar);
        if self.history.len() > 3 {
            self.history.pop_front();
        }
        if self.history.len() < 3 {
            return Ok(SignalValue::Unavailable);
        }

        for b in &self.history {
            if !self.is_strong_bullish(b)? {
                return Ok(SignalValue::Scalar(Decimal::ZERO));
            }
        }

        // Each bar should also open within the previous bar's body (or higher)
        let b1 = &self.history[0];
        let b2 = &self.history[1];
        let b3 = &self.history[2];

        // Bar 2 opens within bar 1's body
        if b2.open < b1.open || b2.open > b1.close {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        // Bar 3 opens within bar 2's body
        if b3.open < b2.open || b3.open > b2.close {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        Ok(SignalValue::Scalar(Decimal::ONE))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= 3
    }

    fn period(&self) -> usize {
        3
    }

    fn reset(&mut self) {
        self.history.clear();
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
        assert!(ThreeWhiteSoldiers::new("tws", 101, 70).is_err());
    }

    #[test]
    fn test_unavailable_before_three_bars() {
        let mut tws = ThreeWhiteSoldiers::new("tws", 60, 70).unwrap();
        assert_eq!(tws.update_bar(&bar("10", "15", "9", "14")).unwrap(), SignalValue::Unavailable);
        assert_eq!(tws.update_bar(&bar("12", "17", "11", "16")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_three_white_soldiers_detected() {
        let mut tws = ThreeWhiteSoldiers::new("tws", 60, 70).unwrap();
        // Bar1: open=10, low=9, high=15, close=14 — body=4/6=67%, close_pct=(14-9)/6=83%
        tws.update_bar(&bar("10", "15", "9", "14")).unwrap();
        // Bar2: open=12 (within 10..14), low=11, high=17, close=16 — body=4/6=67%, close=16-11=5/6=83%
        tws.update_bar(&bar("12", "17", "11", "16")).unwrap();
        // Bar3: open=14 (within 12..16), low=13, high=20, close=19 — body=5/7=71%, close=19-13=6/7=86%
        let v = tws.update_bar(&bar("14", "20", "13", "19")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bearish_bar_breaks_pattern() {
        let mut tws = ThreeWhiteSoldiers::new("tws", 60, 70).unwrap();
        tws.update_bar(&bar("10", "15", "9", "14")).unwrap();
        tws.update_bar(&bar("12", "17", "11", "16")).unwrap();
        // Bar3 is bearish
        let v = tws.update_bar(&bar("16", "17", "12", "13")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut tws = ThreeWhiteSoldiers::new("tws", 60, 70).unwrap();
        for _ in 0..3 {
            tws.update_bar(&bar("10", "15", "9", "14")).unwrap();
        }
        assert!(tws.is_ready());
        tws.reset();
        assert!(!tws.is_ready());
    }
}
