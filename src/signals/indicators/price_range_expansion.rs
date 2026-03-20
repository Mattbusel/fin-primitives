//! Price Range Expansion indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling percentage of bars where `range > prior bar range`.
///
/// Measures how often volatility is expanding bar-over-bar.
/// High values suggest accelerating volatility; low values suggest compression.
pub struct PriceRangeExpansion {
    period: usize,
    prev_range: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl PriceRangeExpansion {
    /// Creates a new `PriceRangeExpansion` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_range: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for PriceRangeExpansion {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if let Some(pr) = self.prev_range {
            let expanded: u8 = if range > pr { 1 } else { 0 };
            self.window.push_back(expanded);
            self.count += expanded as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_range = Some(range);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let pct = Decimal::from(self.count as u32) / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_range = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "PriceRangeExpansion" }
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
    fn test_range_expansion_always_expanding() {
        let mut sig = PriceRangeExpansion::new(2).unwrap();
        sig.update(&bar("105", "95")).unwrap(); // range=10, seeds prev
        sig.update(&bar("110", "90")).unwrap(); // range=20 > 10 ✓
        let v = sig.update(&bar("115", "85")).unwrap(); // range=30 > 20 ✓ → 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_range_expansion_never_expanding() {
        let mut sig = PriceRangeExpansion::new(2).unwrap();
        sig.update(&bar("120", "80")).unwrap(); // range=40
        sig.update(&bar("110", "90")).unwrap(); // range=20 < 40 ✗
        let v = sig.update(&bar("105", "95")).unwrap(); // range=10 < 20 ✗ → 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
