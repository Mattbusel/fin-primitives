//! Median High-Low Range indicator.
//!
//! Computes the rolling median of `(high - low)` bar ranges, providing an
//! outlier-robust measure of typical bar volatility.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Median of High-Low Range.
///
/// Collects `period` values of `high - low` and returns their median.
/// Unlike the mean (as used by ATR), the median is robust to outlier bars
/// (e.g. gap sessions or news spikes) that would skew the average.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MedianHighLow;
/// use fin_primitives::signals::Signal;
///
/// let mhl = MedianHighLow::new("mhl", 14).unwrap();
/// assert_eq!(mhl.period(), 14);
/// assert!(!mhl.is_ready());
/// ```
pub struct MedianHighLow {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl MedianHighLow {
    /// Constructs a new `MedianHighLow`.
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
            window: VecDeque::with_capacity(period),
        })
    }

    fn median(window: &VecDeque<Decimal>) -> Decimal {
        let mut sorted: Vec<Decimal> = window.iter().copied().collect();
        sorted.sort();
        let n = sorted.len();
        if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / Decimal::from(2u32)
        }
    }
}

impl Signal for MedianHighLow {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();

        self.window.push_back(range);
        if self.window.len() > self.period {
            self.window.pop_front();
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        Ok(SignalValue::Scalar(Self::median(&self.window)))
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
    fn test_mhl_invalid_period() {
        assert!(MedianHighLow::new("mhl", 0).is_err());
    }

    #[test]
    fn test_mhl_unavailable_during_warmup() {
        let mut mhl = MedianHighLow::new("mhl", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(mhl.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_mhl_constant_range() {
        let mut mhl = MedianHighLow::new("mhl", 3).unwrap();
        for _ in 0..3 {
            mhl.update_bar(&bar("110", "90")).unwrap();
        }
        let v = mhl.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_mhl_odd_period_median() {
        let mut mhl = MedianHighLow::new("mhl", 3).unwrap();
        // ranges: 10, 20, 30 → median = 20
        mhl.update_bar(&bar("110", "100")).unwrap();
        mhl.update_bar(&bar("120", "100")).unwrap();
        let v = mhl.update_bar(&bar("130", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_mhl_even_period_median() {
        let mut mhl = MedianHighLow::new("mhl", 4).unwrap();
        // ranges: 10, 20, 30, 40 → median = (20+30)/2 = 25
        mhl.update_bar(&bar("110", "100")).unwrap();
        mhl.update_bar(&bar("120", "100")).unwrap();
        mhl.update_bar(&bar("130", "100")).unwrap();
        let v = mhl.update_bar(&bar("140", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(25)));
    }

    #[test]
    fn test_mhl_outlier_robustness() {
        let mut mhl = MedianHighLow::new("mhl", 3).unwrap();
        // ranges: 10, 10, 1000 → median = 10 (not 340 like mean)
        mhl.update_bar(&bar("110", "100")).unwrap();
        mhl.update_bar(&bar("110", "100")).unwrap();
        let v = mhl.update_bar(&bar("1100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_mhl_reset() {
        let mut mhl = MedianHighLow::new("mhl", 3).unwrap();
        for _ in 0..3 {
            mhl.update_bar(&bar("110", "90")).unwrap();
        }
        assert!(mhl.is_ready());
        mhl.reset();
        assert!(!mhl.is_ready());
    }
}
