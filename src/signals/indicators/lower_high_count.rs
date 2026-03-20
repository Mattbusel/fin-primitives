//! Lower High Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where high < previous bar's high.
///
/// Measures downtrend quality — lower highs indicate sellers capping rallies.
/// High count: strong downtrend with consistent resistance at lower levels.
/// Low count: trend weakening, failing to make lower highs.
pub struct LowerHighCount {
    period: usize,
    prev_high: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl LowerHighCount {
    /// Creates a new `LowerHighCount` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_high: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for LowerHighCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(ph) = self.prev_high {
            let lower: u8 = if bar.high < ph { 1 } else { 0 };
            self.window.push_back(lower);
            self.count += lower as usize;
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
    fn name(&self) -> &str { "LowerHighCount" }
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
    fn test_lhc_all_lower_highs() {
        let mut sig = LowerHighCount::new(3).unwrap();
        sig.update(&bar("120")).unwrap();
        sig.update(&bar("118")).unwrap();
        sig.update(&bar("116")).unwrap();
        let v = sig.update(&bar("114")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_lhc_no_lower_highs() {
        let mut sig = LowerHighCount::new(3).unwrap();
        sig.update(&bar("110")).unwrap();
        sig.update(&bar("112")).unwrap();
        sig.update(&bar("114")).unwrap();
        let v = sig.update(&bar("116")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
