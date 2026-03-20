//! Support Test Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars whose low is within 0.5% of the period's lowest low.
///
/// Measures how many times price has tested the support level in the recent window.
/// Higher counts suggest a stronger, well-tested support zone.
pub struct SupportTestCount {
    period: usize,
    lows: VecDeque<Decimal>,
    threshold_pct: Decimal,
}

impl SupportTestCount {
    /// Creates a new `SupportTestCount` with the given rolling period and threshold percentage.
    pub fn new(period: usize, threshold_pct: Decimal) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, lows: VecDeque::with_capacity(period), threshold_pct })
    }
}

impl Signal for SupportTestCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.lows.push_back(bar.low);
        if self.lows.len() > self.period {
            self.lows.pop_front();
        }
        if self.lows.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let period_low = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let threshold = period_low * self.threshold_pct / Decimal::ONE_HUNDRED;
        let count = self.lows.iter()
            .filter(|&&l| (l - period_low).abs() <= threshold)
            .count();
        Ok(SignalValue::Scalar(Decimal::from(count as u32)))
    }

    fn is_ready(&self) -> bool { self.lows.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.lows.clear(); }
    fn name(&self) -> &str { "SupportTestCount" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(l: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(110),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_support_test_count_all_at_support() {
        let mut sig = SupportTestCount::new(3, dec!(0.5)).unwrap();
        sig.update(&bar("90")).unwrap();
        sig.update(&bar("90")).unwrap();
        let v = sig.update(&bar("90")).unwrap();
        // All 3 at same low => all 3 tests
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_support_test_count_one_test() {
        let mut sig = SupportTestCount::new(3, dec!(0.5)).unwrap();
        sig.update(&bar("90")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("110")).unwrap();
        // Only bar at 90 is the period low, others are far above
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }
}
