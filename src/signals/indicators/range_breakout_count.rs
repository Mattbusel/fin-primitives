//! Range Breakout Count indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Breakout Count — number of bars in the last N bars where the bar's range
/// exceeds the rolling average range.
///
/// ```text
/// avg_range = mean(range, N)
/// breakout  = range_i > avg_range
/// count     = sum(breakout_i for i in last N bars)
/// ```
///
/// - **High count**: many recent bars showing above-average volatility — volatile regime.
/// - **Low count**: mostly narrow bars — calm, compressed market.
/// - The avg_range is recomputed on each bar, so the breakout condition is self-referential
///   (a bar can simultaneously push up the avg and be above/below it).
/// - Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeBreakoutCount;
/// use fin_primitives::signals::Signal;
///
/// let rbc = RangeBreakoutCount::new("rbc", 14).unwrap();
/// assert_eq!(rbc.period(), 14);
/// ```
pub struct RangeBreakoutCount {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
    sum: Decimal,
}

impl RangeBreakoutCount {
    /// Constructs a new `RangeBreakoutCount`.
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
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RangeBreakoutCount {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();

        self.sum += range;
        self.ranges.push_back(range);
        if self.ranges.len() > self.period {
            let removed = self.ranges.pop_front().unwrap();
            self.sum -= removed;
        }

        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let count = self.ranges.iter().filter(|&&r| r > avg).count();

        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(Decimal::from(count as u32)))
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
    fn test_rbc_invalid_period() {
        assert!(RangeBreakoutCount::new("rbc", 0).is_err());
    }

    #[test]
    fn test_rbc_unavailable_during_warmup() {
        let mut rbc = RangeBreakoutCount::new("rbc", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(rbc.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!rbc.is_ready());
    }

    #[test]
    fn test_rbc_uniform_ranges_near_half() {
        // All equal ranges → avg = range → 0 bars strictly above avg
        let mut rbc = RangeBreakoutCount::new("rbc", 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..4 {
            last = rbc.update_bar(&bar("110", "90")).unwrap(); // all range=20
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rbc_alternating_wide_narrow() {
        // 2 wide + 2 narrow → avg = mid → wide bars counted
        let mut rbc = RangeBreakoutCount::new("rbc", 4).unwrap();
        rbc.update_bar(&bar("120", "80")).unwrap(); // range=40
        rbc.update_bar(&bar("120", "80")).unwrap(); // range=40
        rbc.update_bar(&bar("105", "95")).unwrap(); // range=10
        let last = rbc.update_bar(&bar("105", "95")).unwrap(); // range=10
        // avg=(40+40+10+10)/4=25, count(range > 25) = 2
        assert_eq!(last, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_rbc_reset() {
        let mut rbc = RangeBreakoutCount::new("rbc", 3).unwrap();
        for _ in 0..3 { rbc.update_bar(&bar("110", "90")).unwrap(); }
        assert!(rbc.is_ready());
        rbc.reset();
        assert!(!rbc.is_ready());
    }
}
