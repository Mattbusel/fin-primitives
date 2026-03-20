//! Bar Overlap Ratio — fraction of the current bar's range that overlaps with the prior bar.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Bar Overlap Ratio — `overlap(current, prior) / current_range`.
///
/// Measures how much of the current bar's range was already covered by the previous bar:
/// - **1.0**: current range is entirely inside the prior bar's range (inside bar or consolidation).
/// - **0.0**: no overlap — gap or complete range extension beyond prior bar.
/// - **Intermediate**: partial overlap.
///
/// Returns [`SignalValue::Unavailable`] until 2 bars have been seen, or when the current
/// bar range is zero (flat bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarOverlapRatio;
/// use fin_primitives::signals::Signal;
/// let bor = BarOverlapRatio::new("bor");
/// assert_eq!(bor.period(), 1);
/// ```
pub struct BarOverlapRatio {
    name: String,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
}

impl BarOverlapRatio {
    /// Constructs a new `BarOverlapRatio`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_high: None, prev_low: None }
    }
}

impl Signal for BarOverlapRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_high.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            let range = bar.high - bar.low;
            if range.is_zero() {
                Ok(SignalValue::Unavailable)
            } else {
                let overlap_high = bar.high.min(ph);
                let overlap_low = bar.low.max(pl);
                let overlap = (overlap_high - overlap_low).max(Decimal::ZERO);
                let ratio = overlap
                    .checked_div(range)
                    .ok_or(FinError::ArithmeticOverflow)?;
                Ok(SignalValue::Scalar(ratio.clamp(Decimal::ZERO, Decimal::ONE)))
            }
        } else {
            Ok(SignalValue::Unavailable)
        };

        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        result
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bor_first_bar_unavailable() {
        let mut s = BarOverlapRatio::new("bor");
        assert_eq!(s.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(s.is_ready());
    }

    #[test]
    fn test_bor_identical_bars_give_one() {
        let mut s = BarOverlapRatio::new("bor");
        s.update_bar(&bar("110", "90")).unwrap();
        let v = s.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bor_no_overlap_gives_zero() {
        let mut s = BarOverlapRatio::new("bor");
        s.update_bar(&bar("110", "90")).unwrap(); // prior: [90, 110]
        let v = s.update_bar(&bar("130", "115")).unwrap(); // current: [115, 130], no overlap
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bor_inside_bar_gives_one() {
        let mut s = BarOverlapRatio::new("bor");
        s.update_bar(&bar("120", "80")).unwrap(); // prior: [80, 120]
        let v = s.update_bar(&bar("110", "90")).unwrap(); // current: [90, 110] fully inside prior
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bor_partial_overlap() {
        let mut s = BarOverlapRatio::new("bor");
        s.update_bar(&bar("110", "90")).unwrap(); // prior: [90, 110]
        // current: [100, 120] → overlap=[100,110]=10, range=20 → 0.5
        let v = s.update_bar(&bar("120", "100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(0.5)).abs() < dec!(0.0001), "expected 0.5 overlap: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bor_output_in_unit_interval() {
        let mut s = BarOverlapRatio::new("bor");
        let bars = [bar("110","90"), bar("115","85"), bar("108","95"), bar("120","100")];
        for b in &bars {
            if let SignalValue::Scalar(v) = s.update_bar(b).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_bor_reset() {
        let mut s = BarOverlapRatio::new("bor");
        s.update_bar(&bar("110", "90")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
