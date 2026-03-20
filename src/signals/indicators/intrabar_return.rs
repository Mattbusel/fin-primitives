//! Intrabar Return indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Intrabar Return — the percentage return from open to close within a single bar.
///
/// ```text
/// intrabar_return = (close - open) / open × 100
/// ```
///
/// Always ready from the first bar. Returns [`SignalValue::Unavailable`] if `open` is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::IntrabarReturn;
/// use fin_primitives::signals::Signal;
///
/// let ir = IntrabarReturn::new("ir").unwrap();
/// assert_eq!(ir.period(), 1);
/// ```
pub struct IntrabarReturn {
    name: String,
}

impl IntrabarReturn {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for IntrabarReturn {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.open.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar((bar.net_move()) / bar.open * Decimal::ONE_HUNDRED))
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
            open: op, high: cp, low: op, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ir_always_ready() {
        let ir = IntrabarReturn::new("ir").unwrap();
        assert!(ir.is_ready());
    }

    #[test]
    fn test_ir_bullish_bar() {
        let mut ir = IntrabarReturn::new("ir").unwrap();
        // open=100, close=110 → +10%
        assert_eq!(ir.update_bar(&bar("100", "110")).unwrap(), SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_ir_bearish_bar() {
        let mut ir = IntrabarReturn::new("ir").unwrap();
        // open=100, close=90 → -10%
        assert_eq!(ir.update_bar(&bar("100", "90")).unwrap(), SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_ir_doji_bar() {
        let mut ir = IntrabarReturn::new("ir").unwrap();
        assert_eq!(ir.update_bar(&bar("100", "100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }
}
