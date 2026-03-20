//! Range Percent of Close — bar range as a percentage of the close price.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Range Percent of Close — `(high - low) / close * 100`.
///
/// Normalizes bar range by the close price, making it comparable across different price
/// levels (unlike raw ATR):
/// - **High value**: wide bar relative to price level (high relative volatility).
/// - **Near 0**: very narrow bar — consolidation or low volatility.
///
/// Returns [`SignalValue::Unavailable`] when the close is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangePctOfClose;
/// use fin_primitives::signals::Signal;
/// let rpc = RangePctOfClose::new("rpc");
/// assert_eq!(rpc.period(), 1);
/// ```
pub struct RangePctOfClose {
    name: String,
}

impl RangePctOfClose {
    /// Constructs a new `RangePctOfClose`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for RangePctOfClose {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let range = bar.range();
        let pct = range
            .checked_div(bar.close)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct.max(Decimal::ZERO)))
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rpc_basic() {
        let mut s = RangePctOfClose::new("rpc");
        // range=20, close=100 → 20%
        let v = s.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_rpc_flat_bar_zero() {
        let mut s = RangePctOfClose::new("rpc");
        let v = s.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rpc_non_negative() {
        let mut s = RangePctOfClose::new("rpc");
        for (h, l, c) in &[("110","90","105"), ("115","85","100"), ("108","95","102")] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(h, l, c)).unwrap() {
                assert!(v >= dec!(0), "range pct must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_rpc_always_ready() {
        let s = RangePctOfClose::new("rpc");
        assert!(s.is_ready());
        assert_eq!(s.period(), 1);
    }
}
