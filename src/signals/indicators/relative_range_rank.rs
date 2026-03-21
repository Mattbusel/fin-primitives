//! Relative Range Rank indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Relative Range Rank.
///
/// Computes the percentile rank of the current bar's range among the last `period` ranges.
/// Useful for identifying range expansion (high rank) and contraction (low rank) relative
/// to recent history.
///
/// Formula: `rank = count(range_i <= range_t, i in window) / period * 100`
///
/// - Returns a value in [0, 100].
/// - 100: current range is the largest in the window.
/// - 0: current range is the smallest.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RelativeRangeRank;
/// use fin_primitives::signals::Signal;
/// let rrr = RelativeRangeRank::new("rrr_20", 20).unwrap();
/// assert_eq!(rrr.period(), 20);
/// ```
pub struct RelativeRangeRank {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl RelativeRangeRank {
    /// Constructs a new `RelativeRangeRank`.
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

impl Signal for RelativeRangeRank {
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

        let count = self.ranges.iter().filter(|&&r| r <= range).count();
        #[allow(clippy::cast_possible_truncation)]
        let rank = Decimal::from(count as u64)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(rank))
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
        assert!(matches!(RelativeRangeRank::new("rrr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut rrr = RelativeRangeRank::new("rrr", 3).unwrap();
        assert_eq!(rrr.update_bar(&bar("12", "10")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_equal_ranges_all_100() {
        let mut rrr = RelativeRangeRank::new("rrr", 3).unwrap();
        for _ in 0..3 {
            rrr.update_bar(&bar("12", "10")).unwrap(); // range=2
        }
        // Same range = all 3 are <= 2, rank = 3/3*100 = 100
        let v = rrr.update_bar(&bar("12", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_largest_range_is_100() {
        let mut rrr = RelativeRangeRank::new("rrr", 3).unwrap();
        rrr.update_bar(&bar("11", "10")).unwrap(); // range=1
        rrr.update_bar(&bar("12", "10")).unwrap(); // range=2
        rrr.update_bar(&bar("13", "10")).unwrap(); // range=3
        // After 3 bars, current bar is range=3 → rank=100
        let v = rrr.update_bar(&bar("15", "10")).unwrap(); // range=5
        if let SignalValue::Scalar(s) = v {
            assert_eq!(s, dec!(100));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut rrr = RelativeRangeRank::new("rrr", 2).unwrap();
        rrr.update_bar(&bar("12", "10")).unwrap();
        rrr.update_bar(&bar("12", "10")).unwrap();
        assert!(rrr.is_ready());
        rrr.reset();
        assert!(!rrr.is_ready());
    }
}
