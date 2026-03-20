//! Gap Detector indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Gap Detector — measures the gap between the current open and the prior close.
///
/// ```text
/// gap_pct_t = (open_t − close_{t-1}) / close_{t-1} × 100
/// ```
///
/// Positive values indicate a gap-up; negative values indicate a gap-down.
/// Values near zero indicate no significant gap.
///
/// Returns [`SignalValue::Unavailable`] on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GapDetector;
/// use fin_primitives::signals::Signal;
///
/// let g = GapDetector::new("gap").unwrap();
/// assert_eq!(g.period(), 1);
/// ```
pub struct GapDetector {
    name: String,
    prev_close: Option<Decimal>,
    last_gap: Option<Decimal>,
}

impl GapDetector {
    /// Creates a new `GapDetector`.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_close: None, last_gap: None })
    }

    /// Returns the most recent gap percentage.
    pub fn last_gap(&self) -> Option<Decimal> { self.last_gap }
}

impl Signal for GapDetector {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let gap = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(pc) => {
                if pc.is_zero() {
                    self.prev_close = Some(bar.close);
                    return Ok(SignalValue::Unavailable);
                }
                (bar.open - pc) / pc * Decimal::from(100u32)
            }
        };
        self.prev_close = Some(bar.close);
        self.last_gap = Some(gap);
        Ok(SignalValue::Scalar(gap))
    }

    fn is_ready(&self) -> bool { self.last_gap.is_some() }
    fn period(&self) -> usize { 1 }

    fn reset(&mut self) {
        self.prev_close = None;
        self.last_gap = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_oc(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: op, low: op, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_gap_unavailable_first_bar() {
        let mut g = GapDetector::new("g").unwrap();
        assert_eq!(
            g.update_bar(&bar_oc("100", "100")).unwrap(),
            SignalValue::Unavailable
        );
    }

    #[test]
    fn test_gap_up() {
        let mut g = GapDetector::new("g").unwrap();
        g.update_bar(&bar_oc("100", "100")).unwrap(); // prev_close = 100
        // open = 105 → gap = (105-100)/100 * 100 = 5%
        if let SignalValue::Scalar(v) = g.update_bar(&bar_oc("105", "105")).unwrap() {
            assert_eq!(v, dec!(5));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_gap_down() {
        let mut g = GapDetector::new("g").unwrap();
        g.update_bar(&bar_oc("100", "100")).unwrap();
        // open = 90 → gap = (90-100)/100 * 100 = -10%
        if let SignalValue::Scalar(v) = g.update_bar(&bar_oc("90", "90")).unwrap() {
            assert_eq!(v, dec!(-10));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_gap_no_gap() {
        let mut g = GapDetector::new("g").unwrap();
        g.update_bar(&bar_oc("100", "100")).unwrap();
        if let SignalValue::Scalar(v) = g.update_bar(&bar_oc("100", "100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_gap_reset() {
        let mut g = GapDetector::new("g").unwrap();
        g.update_bar(&bar_oc("100", "100")).unwrap();
        g.update_bar(&bar_oc("105", "105")).unwrap();
        assert!(g.is_ready());
        g.reset();
        assert!(!g.is_ready());
        assert!(g.last_gap().is_none());
    }
}
