//! Candle Pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Candle Pattern — detects common single-bar and two-bar price action patterns.
///
/// Returns:
/// * `+2` — Bullish Engulfing (two-bar): current bullish body engulfs prior bearish body
/// * `+1` — Hammer: small body at top of range, long lower wick (≥ 2× body), no upper wick
/// * `-1` — Shooting Star: small body at bottom of range, long upper wick (≥ 2× body)
/// * `-2` — Bearish Engulfing: current bearish body engulfs prior bullish body
/// * `0`  — No pattern detected this bar
///
/// Returns [`SignalValue::Unavailable`] until the second bar (needed for engulfing patterns).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CandlePattern;
/// use fin_primitives::signals::Signal;
///
/// let cp = CandlePattern::new("cp").unwrap();
/// assert_eq!(cp.period(), 2);
/// ```
pub struct CandlePattern {
    name: String,
    prev: Option<BarInput>,
}

impl CandlePattern {
    /// Creates a new `CandlePattern`.
    ///
    /// # Errors
    /// Always succeeds.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev: None })
    }
}

impl Signal for CandlePattern {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let body = (bar.net_move()).abs();

        let prev = self.prev.replace(*bar);

        let signal = if let Some(p) = prev {
            // Two-bar patterns
            let prev_body = (p.close - p.open).abs();
            let curr_bull = bar.close > bar.open;
            let curr_bear = bar.close < bar.open;
            let prev_bull = p.close > p.open;
            let prev_bear = p.close < p.open;

            if curr_bull && prev_bear
                && bar.open <= p.close
                && bar.close >= p.open
                && body > prev_body
            {
                Decimal::from(2i32)  // Bullish engulfing
            } else if curr_bear && prev_bull
                && bar.open >= p.close
                && bar.close <= p.open
                && body > prev_body
            {
                Decimal::from(-2i32)  // Bearish engulfing
            } else {
                // Single-bar patterns (use current bar only)
                let upper_wick = bar.high - bar.close.max(bar.open);
                let lower_wick = bar.open.min(bar.close) - bar.low;
                let body_pct = if range.is_zero() { Decimal::ZERO } else { body / range };

                if range.is_zero() {
                    Decimal::ZERO  // No pattern for doji/flat bars
                } else if body_pct < Decimal::new(3, 1) && lower_wick >= body * Decimal::TWO && upper_wick <= body && lower_wick > Decimal::ZERO {
                    Decimal::ONE   // Hammer
                } else if body_pct < Decimal::new(3, 1) && upper_wick >= body * Decimal::TWO && lower_wick <= body && upper_wick > Decimal::ZERO {
                    -Decimal::ONE  // Shooting star
                } else {
                    Decimal::ZERO
                }
            }
        } else {
            Decimal::ZERO
        };

        if prev.is_none() {
            return Ok(SignalValue::Unavailable);
        }

        Ok(SignalValue::Scalar(signal))
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
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_ohlc(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low:  Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn flat_bar(c: &str) -> OhlcvBar { bar_ohlc(c, c, c, c) }

    #[test]
    fn test_candle_first_bar_unavailable() {
        let mut cp = CandlePattern::new("cp").unwrap();
        assert_eq!(cp.update_bar(&flat_bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_candle_flat_is_zero() {
        let mut cp = CandlePattern::new("cp").unwrap();
        cp.update_bar(&flat_bar("100")).unwrap();
        if let SignalValue::Scalar(v) = cp.update_bar(&flat_bar("100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_bullish_engulfing() {
        let mut cp = CandlePattern::new("cp").unwrap();
        // Prior bearish bar: open=110, close=100
        cp.update_bar(&bar_ohlc("110", "110", "100", "100")).unwrap();
        // Current bullish bar engulfs: open=99, close=111, body > prev body
        if let SignalValue::Scalar(v) = cp.update_bar(&bar_ohlc("99", "111", "99", "111")).unwrap() {
            assert_eq!(v, dec!(2), "bullish engulfing should be +2: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_bearish_engulfing() {
        let mut cp = CandlePattern::new("cp").unwrap();
        // Prior bullish bar: open=100, close=110
        cp.update_bar(&bar_ohlc("100", "110", "100", "110")).unwrap();
        // Current bearish bar engulfs: open=111, close=99, body > prev body
        if let SignalValue::Scalar(v) = cp.update_bar(&bar_ohlc("111", "111", "99", "99")).unwrap() {
            assert_eq!(v, dec!(-2), "bearish engulfing should be -2: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_reset() {
        let mut cp = CandlePattern::new("cp").unwrap();
        cp.update_bar(&flat_bar("100")).unwrap();
        assert!(cp.is_ready());
        cp.reset();
        assert!(!cp.is_ready());
        assert_eq!(cp.update_bar(&flat_bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
