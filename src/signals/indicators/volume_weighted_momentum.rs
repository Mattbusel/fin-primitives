//! Volume-Weighted Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling sum of `(close - prev_close) * volume`.
///
/// Combines price direction with volume weight: bullish bars with high volume
/// contribute positively, bearish bars with high volume contribute negatively.
/// Measures directional volume pressure over the period.
pub struct VolumeWeightedMomentum {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeWeightedMomentum {
    /// Creates a new `VolumeWeightedMomentum` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for VolumeWeightedMomentum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let wm = (bar.close - pc) * bar.volume;
            self.window.push_back(wm);
            self.sum += wm;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeWeightedMomentum" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str, v: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(110),
            low: dec!(90),
            close: c.parse().unwrap(),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vwm_flat_zero() {
        // No price change → momentum = 0
        let mut sig = VolumeWeightedMomentum::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        let v = sig.update(&bar("100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vwm_positive_trend() {
        // Rising closes with volume → positive momentum
        let mut sig = VolumeWeightedMomentum::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap(); // seeds prev_close=100
        sig.update(&bar("102", "1000")).unwrap(); // +2*1000=2000
        let v = sig.update(&bar("104", "1000")).unwrap(); // +2*1000=2000, window=[2000,2000], sum=4000
        assert_eq!(v, SignalValue::Scalar(dec!(4000)));
    }
}
