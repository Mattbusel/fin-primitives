//! Range Expansion Index indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of current range divided by previous range.
///
/// Values > 1 indicate expanding ranges (increasing volatility).
/// Values < 1 indicate contracting ranges (decreasing volatility).
/// Bars with zero previous range are skipped.
pub struct RangeExpansionIndex {
    period: usize,
    prev_range: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl RangeExpansionIndex {
    /// Creates a new `RangeExpansionIndex` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_range: None,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RangeExpansionIndex {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if let Some(pr) = self.prev_range {
            if !pr.is_zero() {
                let ratio = range / pr;
                self.window.push_back(ratio);
                self.sum += ratio;
                if self.window.len() > self.period {
                    if let Some(old) = self.window.pop_front() {
                        self.sum -= old;
                    }
                }
            }
        }
        self.prev_range = Some(range);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_range = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "RangeExpansionIndex" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_rei_constant_range() {
        // Same range each bar → ratio = 1
        let mut sig = RangeExpansionIndex::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_rei_doubling_range() {
        // Range doubles each bar → ratio = 2
        let mut sig = RangeExpansionIndex::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap();   // range=20
        sig.update(&bar("120", "80")).unwrap();   // range=40, ratio=2
        let v = sig.update(&bar("140", "60")).unwrap(); // range=80, ratio=2, avg=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }
}
