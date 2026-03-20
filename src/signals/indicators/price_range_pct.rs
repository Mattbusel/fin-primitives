//! Price Range Percent indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Price Range Percent (PriceRangePct) — the high-low range as a percentage of close.
///
/// ```text
/// PriceRangePct = (high - low) / close × 100
/// ```
///
/// Measures intrabar volatility normalised by price level. A high value indicates
/// a wide-ranging bar; a low value indicates a tight, low-volatility bar.
///
/// Returns [`SignalValue::Unavailable`] if `close` is zero.
/// Always produces a value on the first bar (requires no warmup).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceRangePct;
/// use fin_primitives::signals::Signal;
///
/// let p = PriceRangePct::new("prp").unwrap();
/// assert_eq!(p.period(), 1);
/// ```
pub struct PriceRangePct {
    name: String,
}

impl PriceRangePct {
    /// Constructs a new `PriceRangePct`.
    ///
    /// # Errors
    /// This constructor never fails; the `Result` type is used for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for PriceRangePct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let range = bar.high - bar.low;
        let pct = range
            .checked_div(bar.close)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
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
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_prp_is_ready_immediately() {
        let p = PriceRangePct::new("prp").unwrap();
        assert!(p.is_ready());
    }

    #[test]
    fn test_prp_10_percent_range() {
        let mut p = PriceRangePct::new("prp").unwrap();
        // high=110, low=90, close=100 → range=20, pct=20%
        if let SignalValue::Scalar(v) = p.update_bar(&bar("110", "90", "100")).unwrap() {
            assert_eq!(v, dec!(20));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_prp_flat_bar_zero() {
        let mut p = PriceRangePct::new("prp").unwrap();
        if let SignalValue::Scalar(v) = p.update_bar(&bar("100", "100", "100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_prp_period() {
        let p = PriceRangePct::new("prp").unwrap();
        assert_eq!(p.period(), 1);
    }

    #[test]
    fn test_prp_reset_noop() {
        let mut p = PriceRangePct::new("prp").unwrap();
        p.reset();
        assert!(p.is_ready());
    }
}
