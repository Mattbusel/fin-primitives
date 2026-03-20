//! Open Gap Percent indicator — overnight gap as a percentage of the prior close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Open Gap Percent — measures the overnight gap as a percentage of the previous bar's close.
///
/// ```text
/// gap_pct[t] = (open[t] - close[t-1]) / close[t-1] × 100
/// ```
///
/// Positive values indicate a gap-up; negative values indicate a gap-down.
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior close).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenGapPct;
/// use fin_primitives::signals::Signal;
/// let ogp = OpenGapPct::new("ogp");
/// assert_eq!(ogp.period(), 1);
/// ```
pub struct OpenGapPct {
    name: String,
    prev_close: Option<Decimal>,
}

impl OpenGapPct {
    /// Constructs a new `OpenGapPct`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_close: None }
    }
}

impl Signal for OpenGapPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                if pc.is_zero() {
                    SignalValue::Unavailable
                } else {
                    let gap = (bar.open - pc) / pc * Decimal::ONE_HUNDRED;
                    SignalValue::Scalar(gap)
                }
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
    fn test_ogp_first_bar_unavailable() {
        let mut ogp = OpenGapPct::new("ogp");
        assert_eq!(ogp.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
        assert!(ogp.is_ready());
    }

    #[test]
    fn test_ogp_gap_up() {
        let mut ogp = OpenGapPct::new("ogp");
        ogp.update_bar(&bar("100", "100")).unwrap();
        let v = ogp.update_bar(&bar("110", "112")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_ogp_gap_down() {
        let mut ogp = OpenGapPct::new("ogp");
        ogp.update_bar(&bar("100", "100")).unwrap();
        let v = ogp.update_bar(&bar("90", "88")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_ogp_no_gap() {
        let mut ogp = OpenGapPct::new("ogp");
        ogp.update_bar(&bar("100", "100")).unwrap();
        let v = ogp.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ogp_reset() {
        let mut ogp = OpenGapPct::new("ogp");
        ogp.update_bar(&bar("100", "102")).unwrap();
        assert!(ogp.is_ready());
        ogp.reset();
        assert!(!ogp.is_ready());
        assert_eq!(ogp.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
    }
}
