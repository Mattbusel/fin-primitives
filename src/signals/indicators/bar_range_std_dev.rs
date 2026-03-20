//! Bar Range Standard Deviation indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bar Range Standard Deviation — the population standard deviation of the
/// `high - low` range over the last `period` bars.
///
/// This measures how consistent bar sizes are. A low value indicates uniform
/// range (orderly market); a high value indicates range is erratic.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarRangeStdDev;
/// use fin_primitives::signals::Signal;
///
/// let brsd = BarRangeStdDev::new("brsd", 20).unwrap();
/// assert_eq!(brsd.period(), 20);
/// ```
pub struct BarRangeStdDev {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl BarRangeStdDev {
    /// Constructs a new `BarRangeStdDev`.
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
        })
    }
}

impl Signal for BarRangeStdDev {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.ranges.push_back(range);
        if self.ranges.len() > self.period { self.ranges.pop_front(); }

        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let vals: Vec<f64> = self.ranges.iter().filter_map(|r| r.to_f64()).collect();
        if vals.len() != self.period {
            return Ok(SignalValue::Unavailable);
        }

        let nf = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / nf;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / nf;

        match Decimal::from_f64(var.sqrt()) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
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
    fn test_brsd_invalid_period() {
        assert!(BarRangeStdDev::new("brsd", 0).is_err());
        assert!(BarRangeStdDev::new("brsd", 1).is_err());
    }

    #[test]
    fn test_brsd_unavailable_before_warm_up() {
        let mut brsd = BarRangeStdDev::new("brsd", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(brsd.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_brsd_constant_range_gives_zero() {
        // All bars have the same range → std dev = 0
        let mut brsd = BarRangeStdDev::new("brsd", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = brsd.update_bar(&bar("110", "90")).unwrap(); // range=20
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_brsd_varying_range_positive() {
        let mut brsd = BarRangeStdDev::new("brsd", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for &(h, l) in &[("110", "90"), ("130", "80"), ("105", "100")] {
            last = brsd.update_bar(&bar(h, l)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "varying ranges should give positive std dev: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_brsd_reset() {
        let mut brsd = BarRangeStdDev::new("brsd", 3).unwrap();
        for _ in 0..3 { brsd.update_bar(&bar("110", "90")).unwrap(); }
        assert!(brsd.is_ready());
        brsd.reset();
        assert!(!brsd.is_ready());
    }
}
