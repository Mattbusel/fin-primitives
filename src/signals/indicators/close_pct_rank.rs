//! Close Percentile Rank indicator.
//!
//! Computes the percentile rank of the current close among all closes in the
//! rolling period window, providing a normalized position measure.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Percentile rank of `close` within the rolling `period`-bar window.
///
/// Defined as:
/// ```text
/// rank = count(closes_in_window < current_close) / (period - 1)
/// ```
///
/// Ranges from `0.0` (current close is the lowest in the window) to `1.0`
/// (current close is the highest). When `period == 1` the rank is always `0`.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ClosePctRank;
/// use fin_primitives::signals::Signal;
///
/// let cpr = ClosePctRank::new("cpr", 20).unwrap();
/// assert_eq!(cpr.period(), 20);
/// assert!(!cpr.is_ready());
/// ```
pub struct ClosePctRank {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl ClosePctRank {
    /// Constructs a new `ClosePctRank`.
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
}

impl Signal for ClosePctRank {
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
        self.window.push_back(bar.close);
        if self.window.len() > self.period {
            self.window.pop_front();
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.period == 1 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let current = bar.close;
        let below = self.window.iter().filter(|&&c| c < current).count();

        #[allow(clippy::cast_possible_truncation)]
        let rank = Decimal::from(below as u32)
            .checked_div(Decimal::from((self.period - 1) as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

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

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_cpr_invalid_period() {
        assert!(ClosePctRank::new("cpr", 0).is_err());
    }

    #[test]
    fn test_cpr_unavailable_during_warmup() {
        let mut cpr = ClosePctRank::new("cpr", 3).unwrap();
        assert_eq!(cpr.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(cpr.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cpr_highest_in_window_returns_one() {
        let mut cpr = ClosePctRank::new("cpr", 3).unwrap();
        cpr.update_bar(&bar("90")).unwrap();
        cpr.update_bar(&bar("95")).unwrap();
        // Current close 110 is strictly above both others
        let v = cpr.update_bar(&bar("110")).unwrap();
        // below count = 2, denominator = 2 → rank = 1.0
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cpr_lowest_in_window_returns_zero() {
        let mut cpr = ClosePctRank::new("cpr", 3).unwrap();
        cpr.update_bar(&bar("110")).unwrap();
        cpr.update_bar(&bar("120")).unwrap();
        // Current close 80 is strictly below both others
        let v = cpr.update_bar(&bar("80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cpr_middle_value() {
        let mut cpr = ClosePctRank::new("cpr", 3).unwrap();
        cpr.update_bar(&bar("90")).unwrap();
        cpr.update_bar(&bar("110")).unwrap();
        // Current close 100 is above 90 and below 110 → below count = 1 / 2 = 0.5
        let v = cpr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_cpr_period_one_returns_zero() {
        let mut cpr = ClosePctRank::new("cpr", 1).unwrap();
        let v = cpr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cpr_reset() {
        let mut cpr = ClosePctRank::new("cpr", 3).unwrap();
        for _ in 0..3 {
            cpr.update_bar(&bar("100")).unwrap();
        }
        assert!(cpr.is_ready());
        cpr.reset();
        assert!(!cpr.is_ready());
    }
}
