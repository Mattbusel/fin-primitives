//! Percent Rank indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Percent Rank — the percentage of past closes that are less than or equal to the current close.
///
/// `PR = count(close[i-1..i-n] <= close[i]) / n × 100`
///
/// Returns a value in `[0, 100]`:
/// - 100 → current close is the highest in the window
/// - 0   → current close is the lowest
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PercentRank;
/// use fin_primitives::signals::Signal;
///
/// let pr = PercentRank::new("pr", 14).unwrap();
/// assert_eq!(pr.period(), 14);
/// ```
pub struct PercentRank {
    name: String,
    period: usize,
    values: VecDeque<Decimal>,
}

impl PercentRank {
    /// Constructs a new `PercentRank` indicator.
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
            values: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for PercentRank {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.values.push_back(bar.close);
        if self.values.len() > self.period + 1 {
            self.values.pop_front();
        }

        if self.values.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let current = bar.close;
        // Compare current close against the prior `period` closes (all except the last pushed)
        let count = self.values
            .iter()
            .rev()
            .skip(1)          // skip current bar
            .take(self.period)
            .filter(|&&v| v <= current)
            .count();

        #[allow(clippy::cast_possible_truncation)]
        let pct = Decimal::from(count as u32) / Decimal::from(self.period as u32) * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool {
        self.values.len() >= self.period + 1
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.values.clear();
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
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cl, high: cl, low: cl, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pr_period_zero_fails() {
        assert!(PercentRank::new("pr", 0).is_err());
    }

    #[test]
    fn test_pr_unavailable_before_period() {
        let mut pr = PercentRank::new("pr", 3).unwrap();
        assert_eq!(pr.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!pr.is_ready());
    }

    #[test]
    fn test_pr_max_when_always_rising() {
        // 100, 101, 102, 103 — each close is above all previous
        let mut pr = PercentRank::new("pr", 3).unwrap();
        pr.update_bar(&bar("100")).unwrap();
        pr.update_bar(&bar("101")).unwrap();
        pr.update_bar(&bar("102")).unwrap();
        let v = pr.update_bar(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_pr_zero_when_always_falling() {
        let mut pr = PercentRank::new("pr", 3).unwrap();
        pr.update_bar(&bar("103")).unwrap();
        pr.update_bar(&bar("102")).unwrap();
        pr.update_bar(&bar("101")).unwrap();
        let v = pr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pr_reset() {
        let mut pr = PercentRank::new("pr", 3).unwrap();
        for c in ["100", "101", "102", "103"] {
            pr.update_bar(&bar(c)).unwrap();
        }
        assert!(pr.is_ready());
        pr.reset();
        assert!(!pr.is_ready());
    }
}
