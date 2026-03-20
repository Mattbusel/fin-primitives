//! Price Change Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where the close changed (up or down) from previous bar.
///
/// Measures market activity / choppiness.
/// High values: price moving frequently (active market).
/// Low values: price staying flat (low-liquidity or range-bound market).
pub struct PriceChangeCount {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl PriceChangeCount {
    /// Creates a new `PriceChangeCount` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for PriceChangeCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let changed: u8 = if bar.close != pc { 1 } else { 0 };
            self.window.push_back(changed);
            self.count += changed as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "PriceChangeCount" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_pcc_all_change() {
        // All closes different → count = period
        let mut sig = PriceChangeCount::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_pcc_no_change() {
        // Constant price → count = 0
        let mut sig = PriceChangeCount::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
