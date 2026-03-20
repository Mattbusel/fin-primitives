//! Volume Rate of Change indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Percentage change in volume from N bars ago: `(vol[t] - vol[t-N]) / vol[t-N] * 100`.
///
/// Positive: volume increasing relative to N bars ago (growing interest).
/// Negative: volume decreasing relative to N bars ago (fading interest).
/// Returns Unavailable until N+1 bars have been seen.
pub struct VolumeRateOfChange {
    period: usize,
    window: VecDeque<Decimal>,
}

impl VolumeRateOfChange {
    /// Creates a new `VolumeRateOfChange` with the given N-bar look-back.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period + 1) })
    }
}

impl Signal for VolumeRateOfChange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        if self.window.len() > self.period + 1 {
            self.window.pop_front();
        }
        if self.window.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        let base = *self.window.front().unwrap();
        if base.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let current = *self.window.back().unwrap();
        Ok(SignalValue::Scalar((current - base) / base * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "VolumeRateOfChange" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(v: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(110),
            low: dec!(90),
            close: dec!(100),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vroc_flat_zero() {
        let mut sig = VolumeRateOfChange::new(2).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        let v = sig.update(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vroc_double_volume() {
        // vol goes from 1000 to 2000 over 2 bars → +100%
        let mut sig = VolumeRateOfChange::new(2).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1500")).unwrap();
        let v = sig.update(&bar("2000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }
}
