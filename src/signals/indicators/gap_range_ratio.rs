//! Gap Range Ratio — fraction of true range attributable to the overnight/session gap.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Gap Range Ratio — `gap_extension / true_range`.
///
/// Measures what fraction of the bar's true range is due to the gap from prior close:
///
/// ```text
/// gap_extension = max(0, prev_close - low)         // gap-down extension
///               + max(0, high - prev_close)         // gap-up extension beyond intrabar
/// true_range    = max(high, prev_close) - min(low, prev_close)
/// ```
///
/// Wait — simpler: gap contribution = `|true_range - bar_range| / true_range`.
///
/// - **0.0**: no gap — TR equals bar range.
/// - **1.0**: entire true range is from the gap (flat intrabar, large gap).
///
/// Returns [`SignalValue::Unavailable`] until 2 bars have been seen, or when
/// true range is zero (completely flat).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GapRangeRatio;
/// use fin_primitives::signals::Signal;
/// let grr = GapRangeRatio::new("grr");
/// assert_eq!(grr.period(), 1);
/// ```
pub struct GapRangeRatio {
    name: String,
    prev_close: Option<Decimal>,
}

impl GapRangeRatio {
    /// Constructs a new `GapRangeRatio`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_close: None }
    }
}

impl Signal for GapRangeRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(pc) = self.prev_close {
            let bar_range = bar.high - bar.low;
            let tr_high = bar.high.max(pc);
            let tr_low = bar.low.min(pc);
            let true_range = tr_high - tr_low;

            if true_range.is_zero() {
                Ok(SignalValue::Unavailable)
            } else {
                let gap_contrib = if true_range > bar_range {
                    true_range - bar_range
                } else {
                    Decimal::ZERO
                };
                let ratio = gap_contrib
                    .checked_div(true_range)
                    .ok_or(FinError::ArithmeticOverflow)?;
                Ok(SignalValue::Scalar(ratio.clamp(Decimal::ZERO, Decimal::ONE)))
            }
        } else {
            Ok(SignalValue::Unavailable)
        };

        self.prev_close = Some(bar.close);
        result
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

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_grr_first_bar_unavailable() {
        let mut s = GapRangeRatio::new("grr");
        assert_eq!(s.update_bar(&bar("100","110","90","102")).unwrap(), SignalValue::Unavailable);
        assert!(s.is_ready());
    }

    #[test]
    fn test_grr_no_gap_gives_zero() {
        let mut s = GapRangeRatio::new("grr");
        s.update_bar(&bar("100","110","90","100")).unwrap(); // close=100
        // Next bar: prev_close=100 inside [90,110] → no gap extension → TR=bar_range
        let v = s.update_bar(&bar("98","112","88","105")).unwrap(); // range=24, TR=24
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_grr_gap_up_contribution() {
        let mut s = GapRangeRatio::new("grr");
        s.update_bar(&bar("100","110","90","100")).unwrap(); // close=100
        // Gap up: low=110 > prev_close=100 → bar_range=10, TR=120-100=20 → gap=10/20=0.5
        let v = s.update_bar(&bar("112","120","110","115")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(0.5)).abs() < dec!(0.0001), "expected 0.5 gap ratio: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_grr_output_in_unit_interval() {
        let mut s = GapRangeRatio::new("grr");
        let bars = [
            bar("100","110","90","100"),
            bar("102","112","92","105"),
            bar("115","120","112","117"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = s.update_bar(b).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_grr_reset() {
        let mut s = GapRangeRatio::new("grr");
        s.update_bar(&bar("100","110","90","102")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
