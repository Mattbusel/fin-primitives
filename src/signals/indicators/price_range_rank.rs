//! Price Range Rank — percentile rank of current bar range vs prior N bar ranges.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Range Rank — percentile rank of the current bar's `(high - low)` within the
/// last `period` bars' ranges, including the current bar.
///
/// Output in `[0, 1]`:
/// - **1.0**: widest bar in the window (highest range rank).
/// - **0.0**: narrowest bar.
/// - **0.5**: exactly the median range.
///
/// Uses `count(past_range < current_range) / (period - 1)` (open-ended lower bound).
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceRangeRank;
/// use fin_primitives::signals::Signal;
/// let prr = PriceRangeRank::new("prr_14", 14).unwrap();
/// assert_eq!(prr.period(), 14);
/// ```
pub struct PriceRangeRank {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl PriceRangeRank {
    /// Constructs a new `PriceRangeRank`.
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

impl Signal for PriceRangeRank {
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

        // Current range is the last element; compare against all others
        let count_below = self.ranges
            .iter()
            .take(self.period - 1) // exclude current bar
            .filter(|&&r| r < range)
            .count() as u32;

        let rank = Decimal::from(count_below)
            .checked_div(Decimal::from((self.period - 1) as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(rank.clamp(Decimal::ZERO, Decimal::ONE)))
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
    fn test_prr_invalid_period() {
        assert!(PriceRangeRank::new("prr", 0).is_err());
        assert!(PriceRangeRank::new("prr", 1).is_err());
    }

    #[test]
    fn test_prr_unavailable_before_period() {
        let mut s = PriceRangeRank::new("prr", 3).unwrap();
        assert_eq!(s.update_bar(&bar("110","90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("110","90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_prr_widest_bar_gives_one() {
        let mut s = PriceRangeRank::new("prr", 3).unwrap();
        s.update_bar(&bar("105","95")).unwrap(); // range=10
        s.update_bar(&bar("106","96")).unwrap(); // range=10
        // Very wide current bar: range=40, both prior ranges=10 → rank=1.0
        let v = s.update_bar(&bar("120","80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_prr_narrowest_bar_gives_zero() {
        let mut s = PriceRangeRank::new("prr", 3).unwrap();
        s.update_bar(&bar("120","80")).unwrap(); // range=40
        s.update_bar(&bar("115","85")).unwrap(); // range=30
        // Narrow current bar: range=4 → rank=0.0
        let v = s.update_bar(&bar("102","98")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_prr_output_in_unit_interval() {
        let mut s = PriceRangeRank::new("prr", 4).unwrap();
        let bars = [
            bar("110","90"), bar("115","85"), bar("108","95"),
            bar("120","80"), bar("106","98"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = s.update_bar(b).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_prr_reset() {
        let mut s = PriceRangeRank::new("prr", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("110","90")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
