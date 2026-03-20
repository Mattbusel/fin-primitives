//! Pivot Distance indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Pivot Distance — distance of the current close from the classic pivot point,
/// expressed as a percentage of the pivot.
///
/// ```text
/// pivot   = (high_{t-1} + low_{t-1} + close_{t-1}) / 3
/// output  = (close_t − pivot) / pivot × 100
/// ```
///
/// Positive values indicate price is above the prior-bar pivot (bullish bias).
/// Negative values indicate price is below the pivot (bearish bias).
///
/// Returns [`SignalValue::Unavailable`] on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PivotDistance;
/// use fin_primitives::signals::Signal;
///
/// let pd = PivotDistance::new("pd").unwrap();
/// assert_eq!(pd.period(), 1);
/// ```
pub struct PivotDistance {
    name: String,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    prev_close: Option<Decimal>,
    last_pivot: Option<Decimal>,
}

impl PivotDistance {
    /// Creates a new `PivotDistance`.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            prev_high: None,
            prev_low: None,
            prev_close: None,
            last_pivot: None,
        })
    }

    /// Returns the most recent pivot value.
    pub fn last_pivot(&self) -> Option<Decimal> { self.last_pivot }
}

impl Signal for PivotDistance {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match (self.prev_high, self.prev_low, self.prev_close) {
            (Some(ph), Some(pl), Some(pc)) => {
                let pivot = (ph + pl + pc) / Decimal::from(3u32);
                self.last_pivot = Some(pivot);
                if pivot.is_zero() {
                    Ok(SignalValue::Scalar(Decimal::ZERO))
                } else {
                    let dist = (bar.close - pivot) / pivot * Decimal::from(100u32);
                    Ok(SignalValue::Scalar(dist))
                }
            }
            _ => Ok(SignalValue::Unavailable),
        };
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        self.prev_close = Some(bar.close);
        result
    }

    fn is_ready(&self) -> bool { self.last_pivot.is_some() }
    fn period(&self) -> usize { 1 }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
        self.prev_close = None;
        self.last_pivot = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hlc(h: &str, l: &str, c: &str) -> OhlcvBar {
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
    fn test_pd_unavailable_first_bar() {
        let mut p = PivotDistance::new("p").unwrap();
        assert_eq!(
            p.update_bar(&bar_hlc("105", "95", "100")).unwrap(),
            SignalValue::Unavailable
        );
    }

    #[test]
    fn test_pd_at_pivot_is_zero() {
        let mut p = PivotDistance::new("p").unwrap();
        // bar1: h=105, l=95, c=100 → pivot=(105+95+100)/3=100
        p.update_bar(&bar_hlc("105", "95", "100")).unwrap();
        // bar2: close=100 = pivot → distance=0
        if let SignalValue::Scalar(v) = p.update_bar(&bar_hlc("100", "100", "100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pd_above_pivot_positive() {
        let mut p = PivotDistance::new("p").unwrap();
        // pivot = (110+90+100)/3 = 100
        p.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        // close = 105 → dist = (105-100)/100 * 100 = 5%
        if let SignalValue::Scalar(v) = p.update_bar(&bar_hlc("105", "105", "105")).unwrap() {
            assert_eq!(v, dec!(5));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pd_below_pivot_negative() {
        let mut p = PivotDistance::new("p").unwrap();
        p.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        // close = 95 → dist = (95-100)/100 * 100 = -5%
        if let SignalValue::Scalar(v) = p.update_bar(&bar_hlc("95", "95", "95")).unwrap() {
            assert_eq!(v, dec!(-5));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pd_pivot_accessor() {
        let mut p = PivotDistance::new("p").unwrap();
        p.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        p.update_bar(&bar_hlc("100", "100", "100")).unwrap();
        // pivot = (110+90+100)/3 = 100
        assert_eq!(p.last_pivot(), Some(dec!(100)));
    }

    #[test]
    fn test_pd_reset() {
        let mut p = PivotDistance::new("p").unwrap();
        p.update_bar(&bar_hlc("105", "95", "100")).unwrap();
        p.update_bar(&bar_hlc("100", "100", "100")).unwrap();
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
        assert!(p.last_pivot().is_none());
    }
}
