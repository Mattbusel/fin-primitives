//! Open vs Previous Close indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Open vs Previous Close.
///
/// Measures the overnight gap as a signed percentage:
/// `gap_pct = (open_t − close_{t−1}) / close_{t−1} × 100`
///
/// - Positive values indicate a gap-up open.
/// - Negative values indicate a gap-down open.
/// - Zero when the open equals the previous close (no gap).
///
/// Returns `SignalValue::Unavailable` on the first bar (no previous close).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenVsPrevClose;
/// use fin_primitives::signals::Signal;
/// let ovc = OpenVsPrevClose::new("ovc").unwrap();
/// assert_eq!(ovc.period(), 2);
/// ```
pub struct OpenVsPrevClose {
    name: String,
    prev_close: Option<Decimal>,
}

impl OpenVsPrevClose {
    /// Constructs a new `OpenVsPrevClose`.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_close: None })
    }
}

impl Signal for OpenVsPrevClose {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(pc) = self.prev_close {
            if pc.is_zero() {
                SignalValue::Scalar(Decimal::ZERO)
            } else {
                let gap = bar.open
                    .checked_sub(pc)
                    .ok_or(FinError::ArithmeticOverflow)?;
                let gap_pct = gap
                    .checked_div(pc)
                    .ok_or(FinError::ArithmeticOverflow)?
                    .checked_mul(Decimal::from(100u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                SignalValue::Scalar(gap_pct)
            }
        } else {
            SignalValue::Unavailable
        };

        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn is_ready(&self) -> bool {
        self.prev_close.is_some()
    }

    fn period(&self) -> usize {
        2
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

    fn bar(open: &str, close: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o,
            high: o.max(c),
            low: o.min(c),
            close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_first_bar_unavailable() {
        let mut ovc = OpenVsPrevClose::new("ovc").unwrap();
        assert_eq!(ovc.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ready_after_first_bar() {
        let mut ovc = OpenVsPrevClose::new("ovc").unwrap();
        ovc.update_bar(&bar("100", "105")).unwrap();
        assert!(ovc.is_ready());
    }

    #[test]
    fn test_gap_up() {
        let mut ovc = OpenVsPrevClose::new("ovc").unwrap();
        ovc.update_bar(&bar("100", "100")).unwrap(); // close=100
        let v = ovc.update_bar(&bar("110", "112")).unwrap(); // open=110 (+10%)
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_gap_down() {
        let mut ovc = OpenVsPrevClose::new("ovc").unwrap();
        ovc.update_bar(&bar("100", "100")).unwrap(); // close=100
        let v = ovc.update_bar(&bar("90", "88")).unwrap(); // open=90 (-10%)
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_no_gap() {
        let mut ovc = OpenVsPrevClose::new("ovc").unwrap();
        ovc.update_bar(&bar("100", "110")).unwrap(); // close=110
        let v = ovc.update_bar(&bar("110", "115")).unwrap(); // open=110, no gap
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut ovc = OpenVsPrevClose::new("ovc").unwrap();
        ovc.update_bar(&bar("100", "105")).unwrap();
        assert!(ovc.is_ready());
        ovc.reset();
        assert!(!ovc.is_ready());
    }
}
