//! Range Compression Ratio — current bar's range relative to the maximum N-period range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Compression Ratio — `current_range / max_range_N_bars` in `(0, 1]`.
///
/// Measures how much today's bar range has compressed relative to the largest
/// range seen in the past `period` bars:
/// - **= 1.0**: current range is the largest in the window — full expansion.
/// - **Low (< 0.3)**: current bar is very compressed — tight consolidation.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated,
/// or when the maximum range is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeCompressionRatio;
/// use fin_primitives::signals::Signal;
/// let rcr = RangeCompressionRatio::new("rcr_10", 10).unwrap();
/// assert_eq!(rcr.period(), 10);
/// ```
pub struct RangeCompressionRatio {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl RangeCompressionRatio {
    /// Constructs a new `RangeCompressionRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            ranges: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RangeCompressionRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        self.ranges.push_back(range);
        if self.ranges.len() > self.period {
            self.ranges.pop_front();
        }
        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let max_range = self.ranges.iter().copied().fold(Decimal::ZERO, Decimal::max);
        if max_range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let current_range = *self.ranges.back().unwrap();
        let ratio = current_range
            .checked_div(max_range)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio.max(Decimal::ZERO).min(Decimal::ONE)))
    }

    fn reset(&mut self) {
        self.ranges.clear();
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
    fn test_rcr_invalid_period() {
        assert!(RangeCompressionRatio::new("rcr", 0).is_err());
    }

    #[test]
    fn test_rcr_unavailable_before_period() {
        let mut s = RangeCompressionRatio::new("rcr", 3).unwrap();
        assert_eq!(s.update_bar(&bar("110","90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("110","90")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_rcr_uniform_ranges_give_one() {
        // All bars same range → current is max → ratio = 1
        let mut s = RangeCompressionRatio::new("rcr", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("110","90")).unwrap(); }
        assert_eq!(s.update_bar(&bar("110","90")).unwrap(), SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_rcr_narrow_after_wide_gives_low_ratio() {
        // Wide bars then narrow → ratio < 1
        let mut s = RangeCompressionRatio::new("rcr", 3).unwrap();
        s.update_bar(&bar("120","80")).unwrap(); // range=40
        s.update_bar(&bar("115","85")).unwrap(); // range=30
        if let SignalValue::Scalar(v) = s.update_bar(&bar("101","99")).unwrap() { // range=2
            assert!(v < dec!(0.1), "narrow bar after wide should give low ratio: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rcr_in_range_zero_to_one() {
        let mut s = RangeCompressionRatio::new("rcr", 3).unwrap();
        for (h,l) in &[("110","90"),("115","85"),("108","92"),("112","88"),("101","99")] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(h,l)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "ratio out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_rcr_reset() {
        let mut s = RangeCompressionRatio::new("rcr", 2).unwrap();
        s.update_bar(&bar("110","90")).unwrap();
        s.update_bar(&bar("110","90")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
