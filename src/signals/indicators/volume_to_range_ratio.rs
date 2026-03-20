//! Volume to Range Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `volume / (high - low)`.
///
/// Measures how much volume is required to move price per unit of range.
/// High values: heavy volume for each unit of range (inefficient, choppy).
/// Low values: price moves wide ranges on light volume (efficient, trending).
/// Bars with zero range are skipped.
pub struct VolumeToRangeRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeToRangeRatio {
    /// Creates a new `VolumeToRangeRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for VolumeToRangeRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if !range.is_zero() {
            let ratio = bar.volume / range;
            self.window.push_back(ratio);
            self.sum += ratio;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
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
    fn name(&self) -> &str { "VolumeToRangeRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, v: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vtrr_basic() {
        // volume=1000, range=20 → ratio=50
        let mut sig = VolumeToRangeRatio::new(2).unwrap();
        sig.update(&bar("110", "90", "1000")).unwrap();
        let v = sig.update(&bar("110", "90", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_vtrr_mixed() {
        // bar1: vol=1000, range=10 → ratio=100; bar2: vol=1000, range=20 → ratio=50; avg=75
        let mut sig = VolumeToRangeRatio::new(2).unwrap();
        sig.update(&bar("110", "100", "1000")).unwrap(); // range=10, ratio=100
        let v = sig.update(&bar("110", "90", "1000")).unwrap(); // range=20, ratio=50, avg=75
        assert_eq!(v, SignalValue::Scalar(dec!(75)));
    }
}
