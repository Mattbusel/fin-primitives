//! Volume Delta Oscillator indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Difference between short-period and long-period volume SMA.
///
/// Positive: short-term volume above long-term baseline (surging activity).
/// Negative: short-term volume below long-term baseline (fading activity).
/// `short_period` must be less than `long_period`.
pub struct VolumeDeltaOscillator {
    short_period: usize,
    long_period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeDeltaOscillator {
    /// Creates a new `VolumeDeltaOscillator` with short and long periods.
    pub fn new(short_period: usize, long_period: usize) -> Result<Self, FinError> {
        if short_period == 0 || long_period <= short_period {
            return Err(FinError::InvalidPeriod(long_period));
        }
        Ok(Self {
            short_period,
            long_period,
            window: VecDeque::with_capacity(long_period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeDeltaOscillator {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        self.sum += bar.volume;
        if self.window.len() > self.long_period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        let long_sma = self.sum / Decimal::from(self.long_period as u32);
        let short_sum: Decimal = self.window.iter().rev().take(self.short_period).sum();
        let short_sma = short_sum / Decimal::from(self.short_period as u32);
        Ok(SignalValue::Scalar(short_sma - long_sma))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.long_period }
    fn period(&self) -> usize { self.long_period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeDeltaOscillator" }
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
    fn test_vdo_equal_volumes_zero() {
        // Constant volume → short SMA = long SMA → oscillator = 0
        let mut sig = VolumeDeltaOscillator::new(2, 4).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        let v = sig.update(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vdo_short_surge() {
        // Long window has lower volumes, short window has high volumes → positive
        let mut sig = VolumeDeltaOscillator::new(2, 4).unwrap();
        sig.update(&bar("500")).unwrap();
        sig.update(&bar("500")).unwrap();
        sig.update(&bar("2000")).unwrap();
        let v = sig.update(&bar("2000")).unwrap();
        // long_sma = (500+500+2000+2000)/4 = 1250, short_sma = (2000+2000)/2 = 2000
        // oscillator = 2000 - 1250 = 750
        assert_eq!(v, SignalValue::Scalar(dec!(750)));
    }
}
