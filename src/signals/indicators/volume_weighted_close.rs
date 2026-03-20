//! Volume Weighted Close indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling volume-weighted average close price (VWAP-like over N bars).
///
/// `Σ(close * volume) / Σ(volume)` over the rolling period.
/// Gives more weight to high-volume bars, smoothing noise from low-volume sessions.
/// Returns Unavailable when total volume in the window is zero.
pub struct VolumeWeightedClose {
    period: usize,
    window: VecDeque<(Decimal, Decimal)>, // (close, volume)
    sum_cv: Decimal,
    sum_v: Decimal,
}

impl VolumeWeightedClose {
    /// Creates a new `VolumeWeightedClose` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            window: VecDeque::with_capacity(period),
            sum_cv: Decimal::ZERO,
            sum_v: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeWeightedClose {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let cv = bar.close * bar.volume;
        self.window.push_back((bar.close, bar.volume));
        self.sum_cv += cv;
        self.sum_v += bar.volume;
        if self.window.len() > self.period {
            if let Some((old_c, old_v)) = self.window.pop_front() {
                self.sum_cv -= old_c * old_v;
                self.sum_v -= old_v;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.sum_v.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum_cv / self.sum_v))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum_cv = Decimal::ZERO; self.sum_v = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeWeightedClose" }
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
    fn test_vwc_equal_volume() {
        // Equal volumes → VWC = simple average
        let mut sig = VolumeWeightedClose::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        let v = sig.update(&bar("110", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(105)));
    }

    #[test]
    fn test_vwc_higher_volume_bar() {
        // Second bar has 4x volume → VWC biased toward 110
        // (100*1 + 110*4) / 5 = (100 + 440) / 5 = 540/5 = 108
        let mut sig = VolumeWeightedClose::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        let v = sig.update(&bar("110", "4000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(108)));
    }
}
