//! Close Above High Previous indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close Above High Previous — outputs `+1` when the current close exceeds the
/// previous bar's high (a bullish breakout), `-1` when the current close falls
/// below the previous bar's low (a bearish breakout), and `0` otherwise.
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no previous high/low).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAboveHighPrev;
/// use fin_primitives::signals::Signal;
///
/// let c = CloseAboveHighPrev::new("c").unwrap();
/// assert_eq!(c.period(), 1);
/// ```
pub struct CloseAboveHighPrev {
    name: String,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
}

impl CloseAboveHighPrev {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_high: None, prev_low: None })
    }
}

impl Signal for CloseAboveHighPrev {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_high.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match (self.prev_high, self.prev_low) {
            (Some(ph), Some(pl)) => {
                if bar.close > ph {
                    SignalValue::Scalar(Decimal::ONE)
                } else if bar.close < pl {
                    SignalValue::Scalar(-Decimal::ONE)
                } else {
                    SignalValue::Scalar(Decimal::ZERO)
                }
            }
            _ => SignalValue::Unavailable,
        };
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
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
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cahp_unavailable_first_bar() {
        let mut c = CloseAboveHighPrev::new("c").unwrap();
        assert_eq!(c.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cahp_bullish_breakout() {
        let mut c = CloseAboveHighPrev::new("c").unwrap();
        c.update_bar(&bar("100", "110", "90", "105")).unwrap();
        let result = c.update_bar(&bar("105", "120", "100", "115")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cahp_bearish_breakout() {
        let mut c = CloseAboveHighPrev::new("c").unwrap();
        c.update_bar(&bar("100", "110", "90", "105")).unwrap();
        let result = c.update_bar(&bar("105", "106", "80", "85")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_cahp_inside_bar() {
        let mut c = CloseAboveHighPrev::new("c").unwrap();
        c.update_bar(&bar("100", "110", "90", "105")).unwrap();
        let result = c.update_bar(&bar("100", "108", "92", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cahp_reset() {
        let mut c = CloseAboveHighPrev::new("c").unwrap();
        c.update_bar(&bar("100", "110", "90", "105")).unwrap();
        c.reset();
        assert_eq!(c.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
    }
}
