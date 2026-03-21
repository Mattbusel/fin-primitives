//! Dark Cloud Cover candlestick pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Dark Cloud Cover two-candle bearish reversal detector.
///
/// The mirror image of the Piercing Line:
/// 1. **Bar 1** — bullish candle.
/// 2. **Bar 2** — bearish candle that opens above the previous close and closes
///    below the midpoint of Bar 1's body.
///
/// Output:
/// - `-1.0` — Dark Cloud Cover detected.
/// - `0.0`  — No pattern.
///
/// Returns `SignalValue::Unavailable` on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DarkCloudCover;
/// use fin_primitives::signals::Signal;
/// let dc = DarkCloudCover::new("dc").unwrap();
/// assert_eq!(dc.period(), 2);
/// ```
pub struct DarkCloudCover {
    name: String,
    prev: Option<BarInput>,
}

impl DarkCloudCover {
    /// Constructs a new `DarkCloudCover` detector.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev: None })
    }
}

impl Signal for DarkCloudCover {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(ref prev) = self.prev {
            // Condition 1: prev bar is bullish
            if prev.close <= prev.open {
                SignalValue::Scalar(Decimal::ZERO)
            } else {
                let prev_body = prev.close - prev.open;
                if prev_body.is_zero() {
                    SignalValue::Scalar(Decimal::ZERO)
                } else {
                    let midpoint = (prev.open + prev.close)
                        .checked_div(Decimal::from(2u32))
                        .ok_or(FinError::ArithmeticOverflow)?;

                    // Condition 2: current bar is bearish
                    // Condition 3: current bar opens above prev close
                    // Condition 4: current bar closes below prev midpoint
                    if bar.close < bar.open
                        && bar.open > prev.close
                        && bar.close < midpoint
                    {
                        SignalValue::Scalar(Decimal::NEGATIVE_ONE)
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
        let mut dc = DarkCloudCover::new("dc").unwrap();
        assert_eq!(dc.update_bar(&bar("10", "21", "9", "20")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_dark_cloud_cover_detected() {
        let mut dc = DarkCloudCover::new("dc").unwrap();
        // Bar1: bullish open=10, close=20, midpoint=15
        dc.update_bar(&bar("10", "21", "9", "20")).unwrap();
        // Bar2: bearish, opens at 21 (above 20), closes at 14 (below midpoint 15)
        let v = dc.update_bar(&bar("21", "22", "13", "14")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_no_pattern_prev_bearish() {
        let mut dc = DarkCloudCover::new("dc").unwrap();
        dc.update_bar(&bar("20", "21", "9", "10")).unwrap();
        let v = dc.update_bar(&bar("21", "22", "13", "14")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_no_pattern_cur_not_bearish() {
        let mut dc = DarkCloudCover::new("dc").unwrap();
        dc.update_bar(&bar("10", "21", "9", "20")).unwrap();
        let v = dc.update_bar(&bar("21", "22", "13", "22")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_no_pattern_cur_not_below_midpoint() {
        let mut dc = DarkCloudCover::new("dc").unwrap();
        // Bar1: open=10, close=20, midpoint=15
        dc.update_bar(&bar("10", "21", "9", "20")).unwrap();
        // Bar2: bearish but closes at 16 (above midpoint 15)
        let v = dc.update_bar(&bar("21", "22", "15", "16")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut dc = DarkCloudCover::new("dc").unwrap();
        dc.update_bar(&bar("10", "21", "9", "20")).unwrap();
        assert!(dc.is_ready());
        dc.reset();
        assert!(!dc.is_ready());
    }
}
