//! Morning Star candlestick pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Morning Star three-candle bullish reversal detector.
///
/// A Morning Star forms at the bottom of a downtrend:
/// 1. **Bar 1** — large bearish candle (strong sellers).
/// 2. **Bar 2** — small body (star) that gaps lower, indicating indecision.
/// 3. **Bar 3** — large bullish candle that closes above the midpoint of Bar 1's body.
///
/// Output:
/// - `1.0` — Morning Star detected on this bar (bar 3 confirms).
/// - `0.0` — No pattern.
///
/// Returns `SignalValue::Unavailable` until 2 bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MorningStar;
/// use fin_primitives::signals::Signal;
/// let ms = MorningStar::new("ms", 30).unwrap();
/// assert_eq!(ms.period(), 3);
/// ```
pub struct MorningStar {
    name: String,
    /// Maximum body size for the "star" bar as % of bar 1 body.
    star_max_pct: Decimal,
    history: VecDeque<BarInput>,
}

impl MorningStar {
    /// Constructs a new `MorningStar` detector.
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

impl Signal for MorningStar {
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

        // Bar 1: bearish
        if b1.close >= b1.open {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let b1_body = b1.open - b1.close;
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

        // Bar 3: bullish, closing above midpoint of bar 1
        if b3.close <= b3.open {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let b1_midpoint = (b1.open + b1.close)
            .checked_div(Decimal::from(2u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        if b3.close <= b1_midpoint {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        Ok(SignalValue::Scalar(Decimal::ONE))
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
        assert!(MorningStar::new("ms", 101).is_err());
    }

    #[test]
    fn test_unavailable_before_two_bars() {
        let mut ms = MorningStar::new("ms", 30).unwrap();
        assert_eq!(ms.update_bar(&bar("20", "21", "9", "10")).unwrap(), SignalValue::Unavailable);
        assert!(!ms.is_ready());
    }

    #[test]
    fn test_ready_after_two_bars() {
        let mut ms = MorningStar::new("ms", 30).unwrap();
        ms.update_bar(&bar("20", "21", "9", "10")).unwrap();
        ms.update_bar(&bar("10", "11", "8", "9")).unwrap();
        assert!(ms.is_ready());
    }

    #[test]
    fn test_morning_star_detected() {
        let mut ms = MorningStar::new("ms", 30).unwrap();
        // Bar1: bearish, body=10 (open=20, close=10)
        ms.update_bar(&bar("20", "21", "9", "10")).unwrap();
        // Bar2: star, body=1 (10% of 10 → ok with 30% threshold)
        ms.update_bar(&bar("9", "10", "8", "9.5")).unwrap();
        // Bar3: bullish, close=17 > midpoint of bar1 (15)
        let v = ms.update_bar(&bar("11", "18", "10", "17")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_no_pattern_bullish_bar1() {
        let mut ms = MorningStar::new("ms", 30).unwrap();
        // Bar1: bullish — not a downtrend
        ms.update_bar(&bar("10", "21", "9", "20")).unwrap();
        ms.update_bar(&bar("9", "10", "8", "9.5")).unwrap();
        let v = ms.update_bar(&bar("11", "18", "10", "17")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_no_pattern_large_star() {
        let mut ms = MorningStar::new("ms", 30).unwrap();
        // Bar1: bearish body=10
        ms.update_bar(&bar("20", "21", "9", "10")).unwrap();
        // Bar2: large star, body=5 (50% of 10 > 30% threshold)
        ms.update_bar(&bar("10", "16", "8", "15")).unwrap();
        let v = ms.update_bar(&bar("14", "21", "13", "20")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut ms = MorningStar::new("ms", 30).unwrap();
        ms.update_bar(&bar("20", "21", "9", "10")).unwrap();
        ms.update_bar(&bar("9", "10", "8", "9.5")).unwrap();
        assert!(ms.is_ready());
        ms.reset();
        assert!(!ms.is_ready());
    }
}
