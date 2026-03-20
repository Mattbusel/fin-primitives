//! Higher Low Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where low > previous bar's low.
///
/// Measures uptrend quality — higher lows indicate buyers supporting the market.
/// High count: strong uptrend with consistent demand at higher levels.
/// Low count: trend weakening, failing to make higher lows.
pub struct HigherLowCount {
    period: usize,
    prev_low: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl HigherLowCount {
    /// Creates a new `HigherLowCount` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_low: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for HigherLowCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pl) = self.prev_low {
            let higher: u8 = if bar.low > pl { 1 } else { 0 };
            self.window.push_back(higher);
            self.count += higher as usize;
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
    fn name(&self) -> &str { "HigherLowCount" }
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
    fn test_hlc_all_higher_lows() {
        let mut sig = HigherLowCount::new(3).unwrap();
        sig.update(&bar("90")).unwrap();
        sig.update(&bar("92")).unwrap();
        sig.update(&bar("94")).unwrap();
        let v = sig.update(&bar("96")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_hlc_no_higher_lows() {
        let mut sig = HigherLowCount::new(3).unwrap();
        sig.update(&bar("96")).unwrap();
        sig.update(&bar("94")).unwrap();
        sig.update(&bar("92")).unwrap();
        let v = sig.update(&bar("90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
