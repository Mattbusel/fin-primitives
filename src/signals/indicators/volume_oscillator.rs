//! Volume Oscillator indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Volume Oscillator: `(fast_ema_vol - slow_ema_vol) / slow_ema_vol * 100`.
///
/// Measures whether short-term volume is expanding or contracting
/// relative to long-term volume. Positive = volume expansion, negative = contraction.
pub struct VolumeOscillator {
    slow_period: usize,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    fast_k: Decimal,
    slow_k: Decimal,
    bars_seen: usize,
}

impl VolumeOscillator {
    /// Creates a new `VolumeOscillator` with the given fast and slow EMA periods.
    pub fn new(fast_period: usize, slow_period: usize) -> Result<Self, FinError> {
        if fast_period == 0 || slow_period == 0 || fast_period >= slow_period {
            return Err(FinError::InvalidPeriod(slow_period));
        }
        let fast_k = Decimal::TWO / (Decimal::from(fast_period as u32) + Decimal::ONE);
        let slow_k = Decimal::TWO / (Decimal::from(slow_period as u32) + Decimal::ONE);
        Ok(Self {
            slow_period,
            fast_ema: None,
            slow_ema: None,
            fast_k,
            slow_k,
            bars_seen: 0,
        })
    }
}

impl Signal for VolumeOscillator {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;
        self.fast_ema = Some(match self.fast_ema {
            None => vol,
            Some(prev) => vol * self.fast_k + prev * (Decimal::ONE - self.fast_k),
        });
        self.slow_ema = Some(match self.slow_ema {
            None => vol,
            Some(prev) => vol * self.slow_k + prev * (Decimal::ONE - self.slow_k),
        });
        self.bars_seen += 1;
        if self.bars_seen < self.slow_period {
            return Ok(SignalValue::Unavailable);
        }
        let slow = self.slow_ema.unwrap();
        if slow.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let fast = self.fast_ema.unwrap();
        Ok(SignalValue::Scalar((fast - slow) / slow * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.bars_seen >= self.slow_period }
    fn period(&self) -> usize { self.slow_period }
    fn reset(&mut self) {
        self.fast_ema = None;
        self.slow_ema = None;
        self.bars_seen = 0;
    }
    fn name(&self) -> &str { "VolumeOscillator" }
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
    fn test_volume_oscillator_not_ready() {
        let mut sig = VolumeOscillator::new(3, 6).unwrap();
        for _ in 0..5 {
            assert_eq!(sig.update(&bar("1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_volume_oscillator_constant_volume() {
        // Constant volume: fast_ema == slow_ema => oscillator = 0
        let mut sig = VolumeOscillator::new(3, 6).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 {
            last = sig.update(&bar("1000")).unwrap();
        }
        if let SignalValue::Scalar(x) = last {
            assert!(x.abs() < dec!(0.0001), "constant vol should give ~0 oscillator, got {}", x);
        }
    }
}
