//! Pivot Point — classic floor trader pivot levels from the prior bar.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Pivot Point — `(high + low + close) / 3` of the prior bar.
///
/// The classic floor-trader pivot is computed from the previous bar's high,
/// low, and close. It acts as a neutral reference level for the current bar:
/// - **Close > Pivot**: bullish bias.
/// - **Close < Pivot**: bearish bias.
/// - **Close ≈ Pivot**: consolidation / indecision.
///
/// Returns [`SignalValue::Unavailable`] until the first bar has been seen
/// (a second bar is needed to produce the first value).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PivotPoint;
/// use fin_primitives::signals::Signal;
/// let pp = PivotPoint::new("pp");
/// assert_eq!(pp.period(), 1);
/// ```
pub struct PivotPoint {
    name: String,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    prev_close: Option<Decimal>,
}

impl PivotPoint {
    /// Constructs a new `PivotPoint`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prev_high: None,
            prev_low: None,
            prev_close: None,
        }
    }
}

impl Signal for PivotPoint {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool {
        self.prev_high.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match (self.prev_high, self.prev_low, self.prev_close) {
            (Some(h), Some(l), Some(c)) => {
                let pivot = (h + l + c)
                    .checked_div(Decimal::from(3u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                SignalValue::Scalar(pivot)
            }
            _ => SignalValue::Unavailable,
        };
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
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
    fn test_pp_first_bar_unavailable() {
        let mut s = PivotPoint::new("pp");
        assert!(!s.is_ready());
        assert_eq!(s.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        // After first bar, ready for next call
        assert!(s.is_ready());
    }

    #[test]
    fn test_pp_second_bar_gives_value() {
        let mut s = PivotPoint::new("pp");
        s.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(s.is_ready());
        // pivot = (110 + 90 + 100) / 3 = 300 / 3 = 100
        let v = s.update_bar(&bar("115", "95", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_pp_known_values() {
        let mut s = PivotPoint::new("pp");
        s.update_bar(&bar("120", "80", "110")).unwrap();
        // pivot = (120 + 80 + 110) / 3 = 310 / 3
        let v = s.update_bar(&bar("130", "95", "120")).unwrap();
        if let SignalValue::Scalar(r) = v {
            let expected = dec!(310) / dec!(3);
            assert!((r - expected).abs() < dec!(0.0001), "unexpected pivot: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pp_reset() {
        let mut s = PivotPoint::new("pp");
        s.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
        assert_eq!(s.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }
}
