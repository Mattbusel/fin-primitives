//! Higher High Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where `high > prior bar high`.
///
/// Measures upside momentum: higher counts indicate persistent upward price exploration.
pub struct HigherHighCount {
    period: usize,
    prev_high: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl HigherHighCount {
    /// Creates a new `HigherHighCount` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_high: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for HigherHighCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(ph) = self.prev_high {
            let hh: u8 = if bar.high > ph { 1 } else { 0 };
            self.window.push_back(hh);
            self.count += hh as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_high = Some(bar.high);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_high = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "HigherHighCount" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: dec!(90),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_higher_high_count_all_higher() {
        let mut sig = HigherHighCount::new(2).unwrap();
        sig.update(&bar("100")).unwrap(); // seeds prev_high=100
        sig.update(&bar("105")).unwrap(); // 105>100 ✓
        let v = sig.update(&bar("110")).unwrap(); // 110>105 ✓ → count=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_higher_high_count_none_higher() {
        let mut sig = HigherHighCount::new(2).unwrap();
        sig.update(&bar("110")).unwrap(); // seeds prev_high=110
        sig.update(&bar("105")).unwrap(); // 105<110 ✗
        let v = sig.update(&bar("100")).unwrap(); // 100<105 ✗ → count=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
