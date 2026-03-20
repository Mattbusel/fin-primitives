//! Lower Low Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where `low < prior bar low`.
///
/// Measures downside momentum: higher counts indicate persistent downward price exploration.
pub struct LowerLowCount {
    period: usize,
    prev_low: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl LowerLowCount {
    /// Creates a new `LowerLowCount` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_low: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for LowerLowCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pl) = self.prev_low {
            let ll: u8 = if bar.low < pl { 1 } else { 0 };
            self.window.push_back(ll);
            self.count += ll as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_low = Some(bar.low);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_low = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "LowerLowCount" }
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
    fn test_lower_low_count_all_lower() {
        let mut sig = LowerLowCount::new(2).unwrap();
        sig.update(&bar("100")).unwrap(); // seeds prev_low=100
        sig.update(&bar("95")).unwrap(); // 95<100 ✓
        let v = sig.update(&bar("90")).unwrap(); // 90<95 ✓ → count=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_lower_low_count_none_lower() {
        let mut sig = LowerLowCount::new(2).unwrap();
        sig.update(&bar("90")).unwrap(); // seeds prev_low=90
        sig.update(&bar("95")).unwrap(); // 95>90 ✗
        let v = sig.update(&bar("100")).unwrap(); // 100>95 ✗ → count=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
