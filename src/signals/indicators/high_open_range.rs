//! High-Open Range indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `high - open` (upper spike from open perspective).
///
/// Measures how far price rallied above the opening price on average.
/// High values: consistent buying pressure from the open, intraday rallies.
/// Low values: price tends to fail near or below the open.
pub struct HighOpenRange {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl HighOpenRange {
    /// Creates a new `HighOpenRange` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for HighOpenRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ho = bar.high - bar.open;
        self.window.push_back(ho);
        self.sum += ho;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "HighOpenRange" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, o: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: h.parse().unwrap(),
            low: dec!(90),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_hor_open_at_high() {
        // open=high → range=0
        let mut sig = HighOpenRange::new(2).unwrap();
        sig.update(&bar("110", "110")).unwrap();
        let v = sig.update(&bar("110", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hor_basic() {
        // high=110, open=100 → ho=10
        let mut sig = HighOpenRange::new(2).unwrap();
        sig.update(&bar("110", "100")).unwrap();
        let v = sig.update(&bar("110", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
