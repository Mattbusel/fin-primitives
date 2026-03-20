//! Median Close Deviation indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Median Close Deviation — measures how far the current close is from the
/// rolling N-bar median close, expressed as a percentage.
///
/// ```text
/// dev = (close - median(closes, n)) / median × 100
/// ```
///
/// Unlike deviation from mean (which is sensitive to outliers), the median is
/// robust. Positive values indicate the price is above the recent median; negative
/// indicates below.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MedianCloseDev;
/// use fin_primitives::signals::Signal;
///
/// let mcd = MedianCloseDev::new("mcd", 14).unwrap();
/// assert_eq!(mcd.period(), 14);
/// ```
pub struct MedianCloseDev {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl MedianCloseDev {
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period),
        })
    }

    fn median(sorted: &[Decimal]) -> Decimal {
        let n = sorted.len();
        if n == 0 { return Decimal::ZERO; }
        if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / Decimal::TWO
        }
    }
}

impl Signal for MedianCloseDev {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }
        if self.closes.len() < self.period { return Ok(SignalValue::Unavailable); }

        let mut sorted: Vec<Decimal> = self.closes.iter().copied().collect();
        sorted.sort();
        let med = Self::median(&sorted);
        if med.is_zero() { return Ok(SignalValue::Unavailable); }
        let dev = (bar.close - med) / med * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(dev))
    }

    fn reset(&mut self) { self.closes.clear(); }
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
    fn test_mcd_invalid() { assert!(MedianCloseDev::new("m", 0).is_err()); }

    #[test]
    fn test_mcd_unavailable() {
        let mut mcd = MedianCloseDev::new("m", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(mcd.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_mcd_same_prices_zero_dev() {
        let mut mcd = MedianCloseDev::new("m", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 { last = mcd.update_bar(&bar("100")).unwrap(); }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_mcd_above_median_positive() {
        // Closes: 95, 100, 110 → median=100, last close=110 → dev=10%
        let mut mcd = MedianCloseDev::new("m", 3).unwrap();
        mcd.update_bar(&bar("95")).unwrap();
        mcd.update_bar(&bar("100")).unwrap();
        let last = mcd.update_bar(&bar("110")).unwrap();
        assert_eq!(last, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_mcd_reset() {
        let mut mcd = MedianCloseDev::new("m", 3).unwrap();
        for _ in 0..3 { mcd.update_bar(&bar("100")).unwrap(); }
        mcd.reset();
        assert!(!mcd.is_ready());
    }
}
