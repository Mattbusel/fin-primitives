//! Piercing Line candlestick pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Piercing Line two-candle bullish reversal detector.
///
/// The Piercing Line is a bullish reversal pattern:
/// 1. **Bar 1** — bearish candle.
/// 2. **Bar 2** — bullish candle that opens below the previous close and closes
///    above the midpoint of Bar 1's body.
///
/// Output:
/// - `1.0` — Piercing Line detected.
/// - `0.0` — No pattern.
///
/// Returns `SignalValue::Unavailable` on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PiercingLine;
/// use fin_primitives::signals::Signal;
/// let pl = PiercingLine::new("pl").unwrap();
/// assert_eq!(pl.period(), 2);
/// ```
pub struct PiercingLine {
    name: String,
    prev: Option<BarInput>,
}

impl PiercingLine {
    /// Constructs a new `PiercingLine` detector.
    ///
    /// # Errors
    /// Never fails; provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev: None })
    }
}

impl Signal for PiercingLine {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(ref prev) = self.prev {
            // Condition 1: prev bar is bearish
            if prev.close >= prev.open {
                SignalValue::Scalar(Decimal::ZERO)
            } else {
                let prev_body = prev.open - prev.close;
                if prev_body.is_zero() {
                    SignalValue::Scalar(Decimal::ZERO)
                } else {
                    let midpoint = (prev.open + prev.close)
                        .checked_div(Decimal::from(2u32))
                        .ok_or(FinError::ArithmeticOverflow)?;

                    // Condition 2: current bar is bullish
                    // Condition 3: current bar opens below prev close
                    // Condition 4: current bar closes above prev midpoint
                    if bar.close > bar.open
                        && bar.open < prev.close
                        && bar.close > midpoint
                    {
                        SignalValue::Scalar(Decimal::ONE)
                    } else {
                        SignalValue::Scalar(Decimal::ZERO)
                    }
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
    fn test_first_bar_unavailable() {
        let mut pl = PiercingLine::new("pl").unwrap();
        assert_eq!(pl.update_bar(&bar("20", "21", "9", "10")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_piercing_line_detected() {
        let mut pl = PiercingLine::new("pl").unwrap();
        // Bar1: bearish open=20, close=10, midpoint=15
        pl.update_bar(&bar("20", "21", "9", "10")).unwrap();
        // Bar2: bullish, opens at 9 (below 10), closes at 16 (above midpoint 15)
        let v = pl.update_bar(&bar("9", "17", "8", "16")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_no_pattern_prev_bullish() {
        let mut pl = PiercingLine::new("pl").unwrap();
        pl.update_bar(&bar("10", "21", "9", "20")).unwrap();
        let v = pl.update_bar(&bar("9", "17", "8", "16")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_no_pattern_cur_not_bullish() {
        let mut pl = PiercingLine::new("pl").unwrap();
        pl.update_bar(&bar("20", "21", "9", "10")).unwrap();
        // Cur bar is bearish
        let v = pl.update_bar(&bar("9", "10", "7", "8")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_no_pattern_cur_not_above_midpoint() {
        let mut pl = PiercingLine::new("pl").unwrap();
        // Bar1: open=20, close=10, midpoint=15
        pl.update_bar(&bar("20", "21", "9", "10")).unwrap();
        // Cur: bullish, opens at 9, but closes at 14 (below midpoint 15)
        let v = pl.update_bar(&bar("9", "15", "8", "14")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut pl = PiercingLine::new("pl").unwrap();
        pl.update_bar(&bar("20", "21", "9", "10")).unwrap();
        assert!(pl.is_ready());
        pl.reset();
        assert!(!pl.is_ready());
    }
}
