//! Range Percentile indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Percentile.
///
/// Computes the percentile of the current bar's range within the last `period` ranges.
/// Similar to Relative Range Rank but uses a 0–1 output instead of 0–100.
///
/// Formula: `percentile = count(range_i < range_t) / (period - 1)`
///
/// This uses the "exclusive" percentile formula (count strictly below, normalized to period-1).
/// Returns 0.0 when period=1 (or all ranges equal).
///
/// - 1.0: current range strictly exceeds all other ranges in the window.
/// - 0.0: current range is the minimum (or period=1).
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangePercentile;
/// use fin_primitives::signals::Signal;
/// let rp = RangePercentile::new("rp_20", 20).unwrap();
/// assert_eq!(rp.period(), 20);
/// ```
pub struct RangePercentile {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl RangePercentile {
    /// Constructs a new `RangePercentile`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, ranges: VecDeque::with_capacity(period) })
    }
}

impl Signal for RangePercentile {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.ranges.push_back(range);
        if self.ranges.len() > self.period {
            self.ranges.pop_front();
        }
        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.period == 1 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let below_count = self.ranges.iter().filter(|&&r| r < range).count();
        #[allow(clippy::cast_possible_truncation)]
        let percentile = Decimal::from(below_count as u64)
            .checked_div(Decimal::from((self.period - 1) as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(percentile))
    }

    fn is_ready(&self) -> bool {
        self.ranges.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
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

    fn bar(high: &str, low: &str) -> OhlcvBar {
        let h = Price::new(high.parse().unwrap()).unwrap();
        let l = Price::new(low.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l, high: h, low: l, close: h,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(RangePercentile::new("rp", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut rp = RangePercentile::new("rp", 3).unwrap();
        assert_eq!(rp.update_bar(&bar("12", "10")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_largest_range_is_one() {
        let mut rp = RangePercentile::new("rp", 4).unwrap();
        rp.update_bar(&bar("11", "10")).unwrap(); // range=1
        rp.update_bar(&bar("12", "10")).unwrap(); // range=2
        rp.update_bar(&bar("13", "10")).unwrap(); // range=3
        // 4th bar: range=5 → 3 strictly below → 3/3 = 1
        let v = rp.update_bar(&bar("15", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_smallest_range_is_zero() {
        let mut rp = RangePercentile::new("rp", 4).unwrap();
        rp.update_bar(&bar("15", "10")).unwrap(); // range=5
        rp.update_bar(&bar("14", "10")).unwrap(); // range=4
        rp.update_bar(&bar("13", "10")).unwrap(); // range=3
        // 4th bar: range=1 → 0 strictly below → 0/3 = 0
        let v = rp.update_bar(&bar("11", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut rp = RangePercentile::new("rp", 2).unwrap();
        rp.update_bar(&bar("12", "10")).unwrap();
        rp.update_bar(&bar("12", "10")).unwrap();
        assert!(rp.is_ready());
        rp.reset();
        assert!(!rp.is_ready());
    }
}
