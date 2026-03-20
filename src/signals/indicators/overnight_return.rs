//! Overnight Return indicator -- return from previous close to current open.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Overnight Return -- the percentage return from the previous bar's close to the
/// current bar's open.
///
/// ```text
/// overnight_return[t] = (open[t] - close[t-1]) / close[t-1] x 100
/// ```
///
/// Distinct from [`crate::signals::indicators::OpenGapPct`] only in name convention;
/// both compute the same overnight gap percentage. This variant is provided for
/// discoverability under the "return" naming convention.
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior close) or if
/// the prior close is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OvernightReturn;
/// use fin_primitives::signals::Signal;
/// let ovr = OvernightReturn::new("ovr");
/// assert_eq!(ovr.period(), 1);
/// ```
pub struct OvernightReturn {
    name: String,
    prev_close: Option<Decimal>,
}

impl OvernightReturn {
    /// Constructs a new `OvernightReturn`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_close: None }
    }
}

impl Signal for OvernightReturn {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) if pc.is_zero() => SignalValue::Unavailable,
            Some(pc) => SignalValue::Scalar((bar.open - pc) / pc * Decimal::ONE_HUNDRED),
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

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let high = if cp.value() > op.value() { cp } else { op };
        let low  = if cp.value() < op.value() { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high, low, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ovr_first_bar_unavailable() {
        let mut ovr = OvernightReturn::new("ovr");
        assert_eq!(ovr.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
        assert!(ovr.is_ready());
    }

    #[test]
    fn test_ovr_gap_up() {
        let mut ovr = OvernightReturn::new("ovr");
        ovr.update_bar(&bar("100", "100")).unwrap(); // close=100
        let v = ovr.update_bar(&bar("110", "112")).unwrap(); // open=110, prev_close=100 -> +10%
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_ovr_gap_down() {
        let mut ovr = OvernightReturn::new("ovr");
        ovr.update_bar(&bar("100", "100")).unwrap();
        let v = ovr.update_bar(&bar("90", "88")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_ovr_no_gap() {
        let mut ovr = OvernightReturn::new("ovr");
        ovr.update_bar(&bar("100", "100")).unwrap();
        let v = ovr.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ovr_reset() {
        let mut ovr = OvernightReturn::new("ovr");
        ovr.update_bar(&bar("100", "102")).unwrap();
        assert!(ovr.is_ready());
        ovr.reset();
        assert!(!ovr.is_ready());
        assert_eq!(ovr.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
    }
}
