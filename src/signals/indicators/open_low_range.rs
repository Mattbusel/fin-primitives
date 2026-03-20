//! Open-Low Range indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `open - low` (lower wick from open perspective).
///
/// Measures how far price fell below the opening price on average.
/// High values: consistent selling pressure from the open, gaps down.
/// Low values: price tends to find support near or above the open.
pub struct OpenLowRange {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenLowRange {
    /// Creates a new `OpenLowRange` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenLowRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ol = bar.open - bar.low;
        self.window.push_back(ol);
        self.sum += ol;
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
    fn name(&self) -> &str { "OpenLowRange" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, l: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: dec!(110),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_olr_open_at_low() {
        // open=low → range=0
        let mut sig = OpenLowRange::new(2).unwrap();
        sig.update(&bar("90", "90")).unwrap();
        let v = sig.update(&bar("90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_olr_basic() {
        // open=100, low=90 → ol=10
        let mut sig = OpenLowRange::new(2).unwrap();
        sig.update(&bar("100", "90")).unwrap();
        let v = sig.update(&bar("100", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
