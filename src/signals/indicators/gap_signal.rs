//! Gap Signal indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Gap Signal — detects the direction of the overnight gap between the previous
/// bar's close and the current bar's open.
///
/// Outputs:
/// - `+1` → gap up (open > prev_close)
/// - `-1` → gap down (open < prev_close)
/// - `0` → no gap (open == prev_close)
///
/// Returns [`SignalValue::Unavailable`] on the very first bar (no prior close).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GapSignal;
/// use fin_primitives::signals::Signal;
///
/// let gs = GapSignal::new("gs").unwrap();
/// assert_eq!(gs.period(), 1);
/// ```
pub struct GapSignal {
    name: String,
    prev_close: Option<Decimal>,
}

impl GapSignal {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_close: None })
    }
}

impl Signal for GapSignal {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                let direction = if bar.open > pc {
                    Decimal::ONE
                } else if bar.open < pc {
                    Decimal::NEGATIVE_ONE
                } else {
                    Decimal::ZERO
                };
                SignalValue::Scalar(direction)
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
    fn test_gs_first_bar_unavailable() {
        let mut gs = GapSignal::new("gs").unwrap();
        assert_eq!(gs.update_bar(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_gs_gap_up() {
        let mut gs = GapSignal::new("gs").unwrap();
        gs.update_bar(&bar("100", "100")).unwrap();
        let r = gs.update_bar(&bar("105", "105")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_gs_gap_down() {
        let mut gs = GapSignal::new("gs").unwrap();
        gs.update_bar(&bar("100", "100")).unwrap();
        let r = gs.update_bar(&bar("95", "95")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_gs_no_gap() {
        let mut gs = GapSignal::new("gs").unwrap();
        gs.update_bar(&bar("100", "105")).unwrap();
        let r = gs.update_bar(&bar("105", "110")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_gs_reset() {
        let mut gs = GapSignal::new("gs").unwrap();
        gs.update_bar(&bar("100", "100")).unwrap();
        assert!(gs.is_ready());
        gs.reset();
        assert!(!gs.is_ready());
    }
}
