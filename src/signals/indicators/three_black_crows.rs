//! Three Black Crows candlestick pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Three Black Crows pattern detector.
///
/// The bearish mirror of Three White Soldiers: three consecutive strong bearish
/// candles, each opening within the prior bar's body and closing near the low.
/// Signals strong downward momentum.
///
/// Criteria for each of the three bars:
/// - Bearish (close < open).
/// - Body ≥ `min_body_pct` of range.
/// - Close ≤ `close_near_low_pct` of range from the low.
///
/// Output:
/// - `-1.0` — Three Black Crows detected.
/// - `0.0`  — No pattern.
///
/// Returns `SignalValue::Unavailable` until 3 bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ThreeBlackCrows;
/// use fin_primitives::signals::Signal;
/// let tbc = ThreeBlackCrows::new("tbc", 60, 30).unwrap();
/// assert_eq!(tbc.period(), 3);
/// ```
pub struct ThreeBlackCrows {
    name: String,
    min_body_pct: Decimal,
    close_near_low_pct: Decimal,
    history: VecDeque<BarInput>,
}

impl ThreeBlackCrows {
    /// Constructs a new `ThreeBlackCrows` detector.
    ///
    /// - `min_body_pct`: minimum body as % of range. Typical: 60.
    /// - `close_near_low_pct`: maximum close position as % from low. Typical: 30.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if percentages are out of range.
    pub fn new(
        name: impl Into<String>,
        min_body_pct: u32,
        close_near_low_pct: u32,
    ) -> Result<Self, FinError> {
        if min_body_pct > 100 || close_near_low_pct > 100 {
            return Err(FinError::InvalidInput("percentage out of range".into()));
        }
        Ok(Self {
            name: name.into(),
            min_body_pct: Decimal::from(min_body_pct),
            close_near_low_pct: Decimal::from(close_near_low_pct),
            history: VecDeque::with_capacity(3),
        })
    }

    fn is_strong_bearish(&self, bar: &BarInput) -> Result<bool, FinError> {
        if bar.close >= bar.open {
            return Ok(false);
        }
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(false);
        }
        let hundred = Decimal::from(100u32);
        let body = bar.open - bar.close;
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
        Ok(body_pct >= self.min_body_pct && close_pct <= self.close_near_low_pct)
    }
}

impl Signal for ThreeBlackCrows {
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
            if !self.is_strong_bearish(b)? {
                return Ok(SignalValue::Scalar(Decimal::ZERO));
            }
        }

        // Each bar opens within the previous bar's body (bearish: open..close is body)
        let b1 = &self.history[0];
        let b2 = &self.history[1];
        let b3 = &self.history[2];

        // Bar 2 opens within bar 1's body range [close, open]
        if b2.open > b1.open || b2.open < b1.close {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        // Bar 3 opens within bar 2's body range [close, open]
        if b3.open > b2.open || b3.open < b2.close {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        Ok(SignalValue::Scalar(Decimal::NEGATIVE_ONE))
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
        assert!(ThreeBlackCrows::new("tbc", 101, 30).is_err());
    }

    #[test]
    fn test_unavailable_before_three_bars() {
        let mut tbc = ThreeBlackCrows::new("tbc", 60, 30).unwrap();
        assert_eq!(tbc.update_bar(&bar("20", "21", "13", "14")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_three_black_crows_detected() {
        let mut tbc = ThreeBlackCrows::new("tbc", 60, 30).unwrap();
        // Bar1: open=20, high=21, low=13, close=14 — body=6/8=75%, close=(14-13)/8=12.5% ✓
        tbc.update_bar(&bar("20", "21", "13", "14")).unwrap();
        // Bar2: open=18 (within 14..20), high=19, low=11, close=12 — body=6/8=75%, close=(12-11)/8=12.5% ✓
        tbc.update_bar(&bar("18", "19", "11", "12")).unwrap();
        // Bar3: open=16 (within 12..18), high=17, low=9, close=10 — body=6/8=75%, close=(10-9)/8=12.5% ✓
        let v = tbc.update_bar(&bar("16", "17", "9", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_bullish_bar_breaks_pattern() {
        let mut tbc = ThreeBlackCrows::new("tbc", 60, 30).unwrap();
        tbc.update_bar(&bar("20", "21", "13", "14")).unwrap();
        tbc.update_bar(&bar("18", "19", "11", "12")).unwrap();
        // Bullish bar
        let v = tbc.update_bar(&bar("12", "19", "11", "18")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut tbc = ThreeBlackCrows::new("tbc", 60, 30).unwrap();
        for _ in 0..3 {
            tbc.update_bar(&bar("20", "21", "13", "14")).unwrap();
        }
        assert!(tbc.is_ready());
        tbc.reset();
        assert!(!tbc.is_ready());
    }
}
