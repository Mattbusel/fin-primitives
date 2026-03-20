//! Volume Accumulation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling sum of signed volume: `+volume` on up bars, `-volume` on down bars.
///
/// Similar to On-Balance Volume (OBV) but windowed over N bars.
/// Positive: net buying pressure over the window.
/// Negative: net selling pressure over the window.
/// Flat bars contribute 0.
pub struct VolumeAccumulation {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeAccumulation {
    /// Creates a new `VolumeAccumulation` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for VolumeAccumulation {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let signed_vol = if bar.close > pc {
                bar.volume
            } else if bar.close < pc {
                -bar.volume
            } else {
                Decimal::ZERO
            };
            self.window.push_back(signed_vol);
            self.sum += signed_vol;
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
    fn name(&self) -> &str { "VolumeAccumulation" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str, v: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_va_all_up() {
        // All up bars → sum = total volume
        let mut sig = VolumeAccumulation::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("101", "1000")).unwrap(); // +1000
        let v = sig.update(&bar("102", "1000")).unwrap(); // +1000, sum=2000
        assert_eq!(v, SignalValue::Scalar(dec!(2000)));
    }

    #[test]
    fn test_va_all_down() {
        // All down bars → sum = -total volume
        let mut sig = VolumeAccumulation::new(2).unwrap();
        sig.update(&bar("102", "1000")).unwrap();
        sig.update(&bar("101", "1000")).unwrap(); // -1000
        let v = sig.update(&bar("100", "1000")).unwrap(); // -1000, sum=-2000
        assert_eq!(v, SignalValue::Scalar(dec!(-2000)));
    }
}
