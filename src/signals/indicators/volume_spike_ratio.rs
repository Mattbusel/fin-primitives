//! Volume Spike Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Ratio of current bar volume to rolling average volume.
///
/// `current_volume / avg_volume` over the rolling period.
/// Values > 1.0: volume above average (potential breakout/climax).
/// Values < 1.0: volume below average (consolidation/quiet).
/// Returns Unavailable when avg_volume is zero.
pub struct VolumeSpikeRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeSpikeRatio {
    /// Creates a new `VolumeSpikeRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for VolumeSpikeRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;
        self.window.push_back(vol);
        self.sum += vol;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let avg = self.sum / Decimal::from(self.period as u32);
        if avg.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(vol / avg))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeSpikeRatio" }
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
    fn test_vsr_average_bar_gives_one() {
        // All same volume → ratio = 1
        let mut sig = VolumeSpikeRatio::new(3).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        let v = sig.update(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_vsr_spike_above_one() {
        // Two bars at 1000, then spike at 3000 → avg=7000/3, ratio > 1
        let mut sig = VolumeSpikeRatio::new(3).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("3000")).unwrap() {
            assert!(v > dec!(1), "spike bar should be > 1, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
