//! Bar Range Consistency — measures how uniform bar ranges are over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bar Range Consistency — `1 - (std_dev(ranges) / mean(ranges))`, clamped to `[0, 1]`.
///
/// Measures how uniform intrabar ranges `(high - low)` are over the last `period` bars:
/// - **Near 1.0**: very consistent bar sizes (low dispersion).
/// - **Near 0.0**: highly variable bar sizes.
/// - **Below 0.0** (clamped to 0): extreme dispersion.
///
/// Uses population standard deviation. Returns [`SignalValue::Unavailable`] until `period`
/// bars have been seen or when mean range is zero (all flat bars).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarRangeConsistency;
/// use fin_primitives::signals::Signal;
/// let brc = BarRangeConsistency::new("brc_10", 10).unwrap();
/// assert_eq!(brc.period(), 10);
/// ```
pub struct BarRangeConsistency {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
    sum: Decimal,
}

impl BarRangeConsistency {
    /// Constructs a new `BarRangeConsistency`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            ranges: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for BarRangeConsistency {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.sum += range;
        self.ranges.push_back(range);
        if self.ranges.len() > self.period {
            let removed = self.ranges.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mean_d = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if mean_d.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let mean_f = mean_d.to_f64().unwrap_or(0.0);
        let n = self.period as f64;
        let variance: f64 = self
            .ranges
            .iter()
            .filter_map(|r| r.to_f64())
            .map(|r| {
                let d = r - mean_f;
                d * d
            })
            .sum::<f64>()
            / n;

        let std_dev = variance.sqrt();
        let cv = std_dev / mean_f;
        let consistency = (1.0 - cv).clamp(0.0, 1.0);

        Decimal::try_from(consistency)
            .map(SignalValue::Scalar)
            .or(Ok(SignalValue::Unavailable))
    }

    fn reset(&mut self) {
        self.ranges.clear();
        self.sum = Decimal::ZERO;
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
    fn test_brc_invalid_period() {
        assert!(BarRangeConsistency::new("brc", 0).is_err());
        assert!(BarRangeConsistency::new("brc", 1).is_err());
    }

    #[test]
    fn test_brc_unavailable_before_period() {
        let mut s = BarRangeConsistency::new("brc", 3).unwrap();
        assert_eq!(s.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_brc_constant_range_gives_one() {
        let mut s = BarRangeConsistency::new("brc", 3).unwrap();
        s.update_bar(&bar("110", "90")).unwrap();
        s.update_bar(&bar("110", "90")).unwrap();
        let v = s.update_bar(&bar("110", "90")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(1)).abs() < dec!(0.0001), "constant ranges → consistency=1: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_brc_output_in_unit_interval() {
        let mut s = BarRangeConsistency::new("brc", 4).unwrap();
        for (h, l) in &[("110","90"),("150","80"),("102","99"),("120","85"),("108","95")] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(h, l)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_brc_reset() {
        let mut s = BarRangeConsistency::new("brc", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("110", "90")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
