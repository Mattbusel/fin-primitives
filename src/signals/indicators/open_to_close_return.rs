//! Open-to-Close Return — percentage move from open to close for the current bar.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Open-to-Close Return — `(close - open) / open * 100`.
///
/// Measures the intrabar directional move as a percentage:
/// - **Positive**: bar closed above its open (bullish bar).
/// - **Negative**: bar closed below its open (bearish bar).
/// - **0**: doji (open == close).
///
/// Returns [`SignalValue::Unavailable`] when the open is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenToCloseReturn;
/// use fin_primitives::signals::Signal;
/// let otcr = OpenToCloseReturn::new("otcr");
/// assert_eq!(otcr.period(), 1);
/// ```
pub struct OpenToCloseReturn {
    name: String,
}

impl OpenToCloseReturn {
    /// Constructs a new `OpenToCloseReturn`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for OpenToCloseReturn {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.open.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let ret = (bar.close - bar.open)
            .checked_div(bar.open)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(ret))
    }

    fn reset(&mut self) {}
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
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp.max(op), low: cp.min(op), close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_otcr_bullish_bar() {
        let mut s = OpenToCloseReturn::new("otcr");
        // open=100, close=105 → (5/100)*100 = 5.0
        let v = s.update_bar(&bar("100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_otcr_bearish_bar() {
        let mut s = OpenToCloseReturn::new("otcr");
        // open=100, close=95 → (-5/100)*100 = -5.0
        let v = s.update_bar(&bar("100", "95")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-5)));
    }

    #[test]
    fn test_otcr_doji() {
        let mut s = OpenToCloseReturn::new("otcr");
        let v = s.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_otcr_always_ready() {
        let s = OpenToCloseReturn::new("otcr");
        assert!(s.is_ready());
        assert_eq!(s.period(), 1);
    }

    #[test]
    fn test_otcr_reset_noop() {
        let mut s = OpenToCloseReturn::new("otcr");
        s.reset();
        assert!(s.is_ready());
    }
}
