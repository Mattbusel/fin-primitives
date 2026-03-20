//! Bar Type indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Bar Type — classifies each bar by the ratio of its body to its total range,
/// producing a 5-level directional strength signal.
///
/// Classification (by body/range ratio `r` and body direction):
/// - `+1.0` → strong bullish (close > open, r ≥ 0.6)
/// - `+0.5` → weak bullish (close > open, r < 0.6)
/// - `0.0` → doji or zero-range bar
/// - `-0.5` → weak bearish (close < open, r < 0.6)
/// - `-1.0` → strong bearish (close < open, r ≥ 0.6)
///
/// Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarType;
/// use fin_primitives::signals::Signal;
///
/// let bt = BarType::new("bt").unwrap();
/// assert_eq!(bt.period(), 1);
/// ```
pub struct BarType {
    name: String,
}

impl BarType {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for BarType {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let body = bar.close - bar.open;

        if range.is_zero() || body.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let ratio = body.abs() / range;
        let threshold = Decimal::from_str_exact("0.6").unwrap_or(Decimal::ZERO);

        let level = if body > Decimal::ZERO {
            if ratio >= threshold { Decimal::ONE }
            else { Decimal::from_str_exact("0.5").unwrap_or(Decimal::ZERO) }
        } else if ratio >= threshold {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::from_str_exact("-0.5").unwrap_or(Decimal::ZERO)
        };
        Ok(SignalValue::Scalar(level))
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
    fn test_bt_strong_bull() {
        let mut bt = BarType::new("bt").unwrap();
        // body=8, range=10, ratio=0.8 ≥ 0.6 → +1
        let r = bt.update_bar(&bar("90", "100", "90", "98")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bt_weak_bull() {
        let mut bt = BarType::new("bt").unwrap();
        // body=2, range=10, ratio=0.2 < 0.6 → +0.5
        let r = bt.update_bar(&bar("90", "100", "90", "92")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_bt_strong_bear() {
        let mut bt = BarType::new("bt").unwrap();
        // body=8, range=10, ratio=0.8 ≥ 0.6, bearish → -1
        let r = bt.update_bar(&bar("98", "100", "90", "90")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_bt_doji() {
        let mut bt = BarType::new("bt").unwrap();
        let r = bt.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bt_always_ready() {
        let bt = BarType::new("bt").unwrap();
        assert!(bt.is_ready());
    }
}
