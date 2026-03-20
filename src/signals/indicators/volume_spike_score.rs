//! Volume Spike Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Current volume divided by the rolling average volume.
///
/// Values > 1 indicate above-average volume (potential spike).
/// Values < 1 indicate below-average volume (quiet market).
/// Useful for confirming breakouts or detecting unusual activity.
pub struct VolumeSpikeScore {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeSpikeScore {
    /// Creates a new `VolumeSpikeScore` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for VolumeSpikeScore {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        self.sum += bar.volume;
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
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar(bar.volume / avg))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeSpikeScore" }
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
    fn test_volume_spike_score_average() {
        // All same volume → score = 1
        let mut sig = VolumeSpikeScore::new(3).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        let v = sig.update(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_volume_spike_score_spike() {
        // 2x average volume → score = 2
        let mut sig = VolumeSpikeScore::new(3).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        let v = sig.update(&bar("4000")).unwrap(); // avg=(1000+1000+4000)/3=2000, score=4000/2000=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }
}
