//! Range Persistence indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where current range exceeds previous range.
///
/// Measures how often intraday ranges are expanding.
/// High values suggest sustained volatility expansion.
/// Low values suggest contracting or stable volatility.
pub struct RangePersistence {
    period: usize,
    prev_range: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl RangePersistence {
    /// Creates a new `RangePersistence` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_range: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for RangePersistence {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
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
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_range = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "RangePersistence" }
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
    fn test_rp_all_expanding() {
        // Each bar's range is larger than previous
        let mut sig = RangePersistence::new(3).unwrap();
        sig.update(&bar("110", "90")).unwrap();  // range=20, seeds
        sig.update(&bar("115", "85")).unwrap();  // range=30 > 20 ✓
        sig.update(&bar("120", "80")).unwrap();  // range=40 > 30 ✓
        let v = sig.update(&bar("125", "75")).unwrap(); // range=50 > 40 ✓, count=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_rp_no_expansion() {
        // Each bar's range equals previous (not strictly greater)
        let mut sig = RangePersistence::new(3).unwrap();
        sig.update(&bar("110", "90")).unwrap();  // range=20
        sig.update(&bar("110", "90")).unwrap();  // range=20, not > 20
        sig.update(&bar("110", "90")).unwrap();  // range=20
        let v = sig.update(&bar("110", "90")).unwrap(); // count=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
