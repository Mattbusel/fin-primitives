//! Candle Color indicator — bar direction as a scalar signal.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Candle Color — classifies each bar by its directional bias.
///
/// ```text
/// +1  if close > open  (bullish bar)
/// -1  if close < open  (bearish bar)
///  0  if close == open (doji)
/// ```
///
/// No warm-up required; produces a value on every bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CandleColor;
/// use fin_primitives::signals::Signal;
///
/// let c = CandleColor::new("cc");
/// assert_eq!(c.period(), 1);
/// ```
pub struct CandleColor {
    name: String,
    ready: bool,
}

impl CandleColor {
    /// Creates a new `CandleColor`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ready: false }
    }
}

impl Signal for CandleColor {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.ready = true;
        let value = if bar.close > bar.open {
            Decimal::ONE
        } else if bar.close < bar.open {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::ZERO
        };
        Ok(SignalValue::Scalar(value))
    }

    fn is_ready(&self) -> bool { self.ready }
    fn period(&self) -> usize { 1 }
    fn reset(&mut self) { self.ready = false; }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let hp = if cp >= op { cp } else { op };
        let lp = if cp <= op { cp } else { op };
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
    fn test_cc_bullish() {
        let mut c = CandleColor::new("cc");
        assert_eq!(c.update_bar(&bar("100", "105")).unwrap(), SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cc_bearish() {
        let mut c = CandleColor::new("cc");
        assert_eq!(c.update_bar(&bar("105", "100")).unwrap(), SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_cc_doji() {
        let mut c = CandleColor::new("cc");
        assert_eq!(c.update_bar(&bar("100", "100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cc_ready_after_first_bar() {
        let mut c = CandleColor::new("cc");
        assert!(!c.is_ready());
        c.update_bar(&bar("100", "105")).unwrap();
        assert!(c.is_ready());
    }

    #[test]
    fn test_cc_reset() {
        let mut c = CandleColor::new("cc");
        c.update_bar(&bar("100", "105")).unwrap();
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
