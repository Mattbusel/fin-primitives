//! Range Expansion Count indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Expansion Count.
///
/// Counts the number of bars in the last `period` bars where the current bar's
/// range (high − low) exceeds the average range over those bars. This gives a
/// rough measure of how often price is expanding vs. contracting.
///
/// Formula:
/// - `mean_range = mean(range, period)`
/// - `expansion_count = count(range_i > mean_range, i in window)`
/// - Normalized: `expansion_count / period`
///
/// - High values (> 0.5): frequent range expansion.
/// - Low values (< 0.5): range contraction dominates.
/// - = 0.5: balanced.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeExpansionCount;
/// use fin_primitives::signals::Signal;
/// let rec = RangeExpansionCount::new("rec_20", 20).unwrap();
/// assert_eq!(rec.period(), 20);
/// ```
pub struct RangeExpansionCount {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl RangeExpansionCount {
    /// Constructs a new `RangeExpansionCount`.
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

impl Signal for RangeExpansionCount {
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

        let sum: Decimal = self.ranges.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let mean = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let expand_count = self.ranges.iter().filter(|&&r| r > mean).count();
        #[allow(clippy::cast_possible_truncation)]
        let result = Decimal::from(expand_count as u64)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(result))
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: hi,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(RangeExpansionCount::new("rec", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut rec = RangeExpansionCount::new("rec", 3).unwrap();
        assert_eq!(rec.update_bar(&bar("12", "10")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_equal_ranges_gives_zero() {
        // All same range → none strictly above mean → 0
        let mut rec = RangeExpansionCount::new("rec", 3).unwrap();
        for _ in 0..3 {
            rec.update_bar(&bar("12", "10")).unwrap();
        }
        let v = rec.update_bar(&bar("12", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_mix_of_ranges() {
        let mut rec = RangeExpansionCount::new("rec", 4).unwrap();
        rec.update_bar(&bar("11", "10")).unwrap(); // range=1
        rec.update_bar(&bar("12", "10")).unwrap(); // range=2
        rec.update_bar(&bar("14", "10")).unwrap(); // range=4
        // After 4 bars, window=[1,2,4,8], mean=15/4=3.75, 2 bars > 3.75
        let v = rec.update_bar(&bar("18", "10")).unwrap(); // range=8
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0) && s <= dec!(1));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut rec = RangeExpansionCount::new("rec", 2).unwrap();
        rec.update_bar(&bar("12", "10")).unwrap();
        rec.update_bar(&bar("12", "10")).unwrap();
        assert!(rec.is_ready());
        rec.reset();
        assert!(!rec.is_ready());
    }
}
