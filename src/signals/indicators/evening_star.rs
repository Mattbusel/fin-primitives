//! Evening Star candlestick pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Evening Star three-candle bearish reversal detector.
///
/// A mirror image of the Morning Star, forming at the top of an uptrend:
/// 1. **Bar 1** — large bullish candle (strong buyers).
/// 2. **Bar 2** — small body (star), indicating indecision near the peak.
/// 3. **Bar 3** — large bearish candle closing below the midpoint of Bar 1's body.
///
/// Output:
/// - `-1.0` — Evening Star detected on this bar (bar 3 confirms).
/// - `0.0`  — No pattern.
///
/// Returns `SignalValue::Unavailable` until 2 bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EveningStar;
/// use fin_primitives::signals::Signal;
/// let es = EveningStar::new("es", 30).unwrap();
/// assert_eq!(es.period(), 3);
/// ```
pub struct EveningStar {
    name: String,
    star_max_pct: Decimal,
    history: VecDeque<BarInput>,
}

impl EveningStar {
    /// Constructs a new `EveningStar` detector.
    ///
    /// `star_max_pct`: maximum body size of the star bar as a percentage of the
    /// first bar's body (0–100). Typical: 30.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `star_max_pct > 100`.
    pub fn new(name: impl Into<String>, star_max_pct: u32) -> Result<Self, FinError> {
        if star_max_pct > 100 {
            return Err(FinError::InvalidInput("star_max_pct out of range".into()));
        }
        Ok(Self {
            name: name.into(),
            star_max_pct: Decimal::from(star_max_pct),
            history: VecDeque::with_capacity(3),
        })
    }
}

impl Signal for EveningStar {
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

        let b1 = &self.history[0];
        let b2 = &self.history[1];
        let b3 = &self.history[2];

        // Bar 1: bullish
        if b1.close <= b1.open {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let b1_body = b1.close - b1.open;
        if b1_body.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        // Bar 2: small body (star)
        let b2_body = (b2.close - b2.open).abs();
        let star_pct = b2_body
            .checked_div(b1_body)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        if star_pct > self.star_max_pct {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        // Bar 3: bearish, closing below midpoint of bar 1
        if b3.close >= b3.open {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let b1_midpoint = (b1.open + b1.close)
            .checked_div(Decimal::from(2u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        if b3.close >= b1_midpoint {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        Ok(SignalValue::Scalar(Decimal::NEGATIVE_ONE))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= 2
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
        assert!(EveningStar::new("es", 101).is_err());
    }

    #[test]
    fn test_unavailable_before_two_bars() {
        let mut es = EveningStar::new("es", 30).unwrap();
        assert_eq!(es.update_bar(&bar("10", "21", "9", "20")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_evening_star_detected() {
        let mut es = EveningStar::new("es", 30).unwrap();
        // Bar1: bullish, body=10 (open=10, close=20)
        es.update_bar(&bar("10", "21", "9", "20")).unwrap();
        // Bar2: star, body=0.5 (5% of 10 → ok with 30%)
        es.update_bar(&bar("21", "22", "20", "20.5")).unwrap();
        // Bar3: bearish, close=14 < midpoint of bar1 (15)
        let v = es.update_bar(&bar("19", "20", "13", "14")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_no_pattern_bearish_bar1() {
        let mut es = EveningStar::new("es", 30).unwrap();
        es.update_bar(&bar("20", "21", "9", "10")).unwrap();
        es.update_bar(&bar("9", "10", "8", "9")).unwrap();
        let v = es.update_bar(&bar("8", "9", "6", "7")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_no_pattern_bar3_doesnt_close_below_midpoint() {
        let mut es = EveningStar::new("es", 30).unwrap();
        // Bar1: bullish, open=10, close=20, midpoint=15
        es.update_bar(&bar("10", "21", "9", "20")).unwrap();
        // Bar2: star
        es.update_bar(&bar("21", "22", "20", "20.5")).unwrap();
        // Bar3: bearish but only closes at 16 (above midpoint 15)
        let v = es.update_bar(&bar("19", "20", "15", "16")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut es = EveningStar::new("es", 30).unwrap();
        es.update_bar(&bar("10", "21", "9", "20")).unwrap();
        es.update_bar(&bar("21", "22", "20", "20.5")).unwrap();
        assert!(es.is_ready());
        es.reset();
        assert!(!es.is_ready());
    }
}
