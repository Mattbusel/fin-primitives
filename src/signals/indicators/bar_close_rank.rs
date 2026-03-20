//! Bar Close Rank indicator -- percentile rank of today's close within last N closes.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bar Close Rank -- percentile rank (0-100) of the current close within the last `period` bars.
///
/// A rank of 100 means today's close is the highest close in the window.
/// A rank of 0 means it is the lowest.
///
/// ```text
/// rank[t] = (count of past closes strictly less than close[t]) / (period - 1) x 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
/// When `period == 1`, always returns 50 (single-element rank is undefined; midpoint is used).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarCloseRank;
/// use fin_primitives::signals::Signal;
/// let bcr = BarCloseRank::new("bcr", 10).unwrap();
/// assert_eq!(bcr.period(), 10);
/// ```
pub struct BarCloseRank {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl BarCloseRank {
    /// Constructs a new `BarCloseRank`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self { name: name.into(), period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for BarCloseRank {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        if self.window.len() > self.period { self.window.pop_front(); }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }

        if self.period == 1 {
            return Ok(SignalValue::Scalar(Decimal::from(50u32)));
        }

        let current = bar.close;
        let below = self.window.iter().filter(|&&v| v < current).count();
        #[allow(clippy::cast_possible_truncation)]
        let rank = Decimal::from(below as u32)
            / Decimal::from((self.period - 1) as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(rank))
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bcr_period_0_error() { assert!(BarCloseRank::new("b", 0).is_err()); }

    #[test]
    fn test_bcr_unavailable_before_period() {
        let mut b = BarCloseRank::new("b", 3).unwrap();
        assert_eq!(b.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bcr_highest_close_is_100() {
        // window [90, 95, 100] -- current 100 is highest
        let mut b = BarCloseRank::new("b", 3).unwrap();
        b.update_bar(&bar("90")).unwrap();
        b.update_bar(&bar("95")).unwrap();
        let v = b.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_bcr_lowest_close_is_0() {
        // window [90, 95, 100] -- if current bar is 80 (lowest), rank=0
        let mut b = BarCloseRank::new("b", 3).unwrap();
        b.update_bar(&bar("90")).unwrap();
        b.update_bar(&bar("95")).unwrap();
        // roll in 80, roll out 90 -- window=[95, 100, 80] -- wait, period=3
        // actually: window=[90,95] then push 80 -> [90,95,80], 80<90 and 80<95, below=0
        let v = b.update_bar(&bar("80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bcr_midpoint() {
        // window [90, 100, 95] (period=3), current=95, below=[90]=1, denom=2 => 50
        let mut b = BarCloseRank::new("b", 3).unwrap();
        b.update_bar(&bar("90")).unwrap();
        b.update_bar(&bar("100")).unwrap();
        let v = b.update_bar(&bar("95")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_bcr_reset() {
        let mut b = BarCloseRank::new("b", 2).unwrap();
        b.update_bar(&bar("90")).unwrap();
        b.update_bar(&bar("100")).unwrap();
        assert!(b.is_ready());
        b.reset();
        assert!(!b.is_ready());
    }
}
