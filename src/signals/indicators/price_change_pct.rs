//! Price Change Percent indicator -- bar-over-bar close change as a percentage.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Price Change Percent -- percentage change in close from the previous bar.
///
/// ```text
/// pct_change[t] = (close[t] - close[t-1]) / close[t-1] x 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior close).
/// Returns [`SignalValue::Unavailable`] if the prior close is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceChangePct;
/// use fin_primitives::signals::Signal;
/// let pcp = PriceChangePct::new("pcp");
/// assert_eq!(pcp.period(), 1);
/// ```
pub struct PriceChangePct {
    name: String,
    prev_close: Option<Decimal>,
}

impl PriceChangePct {
    /// Constructs a new `PriceChangePct`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_close: None }
    }
}

impl Signal for PriceChangePct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) if pc.is_zero() => SignalValue::Unavailable,
            Some(pc) => {
                let pct = (bar.close - pc) / pc * Decimal::ONE_HUNDRED;
                SignalValue::Scalar(pct)
            }
        };
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pcp_first_bar_unavailable() {
        let mut pcp = PriceChangePct::new("pcp");
        assert_eq!(pcp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(pcp.is_ready());
    }

    #[test]
    fn test_pcp_gain() {
        let mut pcp = PriceChangePct::new("pcp");
        pcp.update_bar(&bar("100")).unwrap();
        let v = pcp.update_bar(&bar("110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_pcp_loss() {
        let mut pcp = PriceChangePct::new("pcp");
        pcp.update_bar(&bar("100")).unwrap();
        let v = pcp.update_bar(&bar("90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_pcp_no_change() {
        let mut pcp = PriceChangePct::new("pcp");
        pcp.update_bar(&bar("100")).unwrap();
        let v = pcp.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pcp_reset() {
        let mut pcp = PriceChangePct::new("pcp");
        pcp.update_bar(&bar("100")).unwrap();
        assert!(pcp.is_ready());
        pcp.reset();
        assert!(!pcp.is_ready());
        assert_eq!(pcp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
