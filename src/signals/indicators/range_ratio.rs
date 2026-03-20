//! Range Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Ratio — current bar's high-low range divided by the N-period average range.
///
/// ```text
/// range_t    = high_t − low_t
/// avg_range  = mean(range, period)
/// output     = range_t / avg_range
/// ```
///
/// Values > 1 indicate an expanding bar; < 1 indicate a compressed/inside bar.
/// Returns 1 when the average range is zero (flat market).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeRatio;
/// use fin_primitives::signals::Signal;
///
/// let rr = RangeRatio::new("rr", 14).unwrap();
/// assert_eq!(rr.period(), 14);
/// ```
pub struct RangeRatio {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl RangeRatio {
    /// Creates a new `RangeRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            ranges: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RangeRatio {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.ranges.push_back(range);
        if self.ranges.len() > self.period { self.ranges.pop_front(); }
        if self.ranges.len() < self.period { return Ok(SignalValue::Unavailable); }

        let avg = self.ranges.iter().sum::<Decimal>() / Decimal::from(self.period as u32);

        if avg.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ONE));
        }

        Ok(SignalValue::Scalar(range / avg))
    }

    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.ranges.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hl(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(dec!(100)).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn bar(r: &str) -> OhlcvBar {
        // Create bar with given range centered on 100
        let range: rust_decimal::Decimal = r.parse().unwrap();
        let half = range / dec!(2);
        let h = format!("{}", dec!(100) + half);
        let l = format!("{}", dec!(100) - half);
        bar_hl(&h, &l)
    }

    #[test]
    fn test_rr_invalid() {
        assert!(RangeRatio::new("r", 0).is_err());
    }

    #[test]
    fn test_rr_unavailable_before_warmup() {
        let mut r = RangeRatio::new("r", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(r.update_bar(&bar("10")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rr_uniform_is_one() {
        // All bars same range → ratio = 1
        let mut r = RangeRatio::new("r", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = r.update_bar(&bar("10")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_rr_expanding_above_one() {
        // 3 bars of range 10, then 1 bar of range 30 → avg=(10+10+30)/3≈16.67, ratio=30/16.67≈1.8
        let mut r = RangeRatio::new("r", 3).unwrap();
        r.update_bar(&bar("10")).unwrap();
        r.update_bar(&bar("10")).unwrap();
        if let SignalValue::Scalar(v) = r.update_bar(&bar("30")).unwrap() {
            assert!(v > dec!(1), "expected > 1, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_rr_contracting_below_one() {
        // 3 bars of range 30, then 1 bar of range 10 → ratio < 1
        let mut r = RangeRatio::new("r", 3).unwrap();
        r.update_bar(&bar("30")).unwrap();
        r.update_bar(&bar("30")).unwrap();
        if let SignalValue::Scalar(v) = r.update_bar(&bar("10")).unwrap() {
            assert!(v < dec!(1), "expected < 1, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_rr_flat_returns_one() {
        // Flat bars (range=0) → avg=0 → returns 1
        let mut r = RangeRatio::new("r", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = r.update_bar(&bar("0")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_rr_reset() {
        let mut r = RangeRatio::new("r", 3).unwrap();
        for _ in 0..5 { r.update_bar(&bar("10")).unwrap(); }
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
