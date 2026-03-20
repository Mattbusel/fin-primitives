//! Range Efficiency indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Efficiency — ratio of net price displacement to the sum of all bar ranges
/// over the last `period` bars.
///
/// ```text
/// net_move   = |close_now - close_N_ago|
/// total_range = sum(high_i - low_i) for i in last N bars
/// efficiency  = net_move / total_range
/// ```
///
/// - **Near 1.0**: price moved consistently in one direction with tight, efficient bars.
/// - **Near 0.0**: lots of range used, little net movement — choppy, mean-reverting market.
/// - Compared to `DirectionalEfficiency` (which uses bar-to-bar returns), this uses raw
///   bar ranges — it's more sensitive to volatility.
/// - Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
/// - Returns [`SignalValue::Unavailable`] if total range is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeEfficiency;
/// use fin_primitives::signals::Signal;
///
/// let re = RangeEfficiency::new("re", 10).unwrap();
/// assert_eq!(re.period(), 10);
/// ```
pub struct RangeEfficiency {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    ranges: VecDeque<Decimal>,
    range_sum: Decimal,
}

impl RangeEfficiency {
    /// Constructs a new `RangeEfficiency`.
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
            closes: VecDeque::with_capacity(period + 1),
            ranges: VecDeque::with_capacity(period),
            range_sum: Decimal::ZERO,
        })
    }
}

impl Signal for RangeEfficiency {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();

        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }

        self.range_sum += range;
        self.ranges.push_back(range);
        if self.ranges.len() > self.period {
            let removed = self.ranges.pop_front().unwrap();
            self.range_sum -= removed;
        }

        if self.ranges.len() < self.period || self.closes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.range_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let first = *self.closes.front().unwrap();
        let last = *self.closes.back().unwrap();
        let net_move = (last - first).abs();

        let efficiency = net_move
            .checked_div(self.range_sum)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(efficiency))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.ranges.clear();
        self.range_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_re_invalid_period() {
        assert!(RangeEfficiency::new("re", 0).is_err());
    }

    #[test]
    fn test_re_unavailable_during_warmup() {
        let mut re = RangeEfficiency::new("re", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(re.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!re.is_ready());
    }

    #[test]
    fn test_re_monotonic_trend() {
        // Bars: close 90, 100, 110, 120 — each bar high=close, low=prev_close
        // range per bar = 10, net_move = |120-90| = 30, total_range = 30 → eff = 1
        let mut re = RangeEfficiency::new("re", 3).unwrap();
        re.update_bar(&bar("90", "90", "90")).unwrap();   // seed
        re.update_bar(&bar("100", "90", "100")).unwrap();  // range=10, close=100
        re.update_bar(&bar("110", "100", "110")).unwrap(); // range=10, close=110
        let result = re.update_bar(&bar("120", "110", "120")).unwrap(); // range=10, close=120
        assert_eq!(result, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_re_flat_is_unavailable() {
        let mut re = RangeEfficiency::new("re", 3).unwrap();
        for _ in 0..4 {
            let r = re.update_bar(&bar("100", "100", "100")).unwrap();
            if re.is_ready() {
                assert_eq!(r, SignalValue::Unavailable, "zero range → Unavailable");
            }
        }
    }

    #[test]
    fn test_re_reset() {
        let mut re = RangeEfficiency::new("re", 3).unwrap();
        for i in 0u32..4 {
            let c = (100 + i * 10).to_string();
            re.update_bar(&bar(&(100 + i * 10 + 5).to_string(), &c, &c)).unwrap();
        }
        assert!(re.is_ready());
        re.reset();
        assert!(!re.is_ready());
    }
}
