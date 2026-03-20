//! Up-Volume Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling fraction of total volume that occurs on up bars (close > open).
///
/// Values > 0.5 indicate buying pressure dominating. Values < 0.5 indicate selling pressure.
pub struct UpVolumeRatio {
    period: usize,
    window: VecDeque<(Decimal, bool)>, // (volume, is_up)
    total_vol: Decimal,
    up_vol: Decimal,
}

impl UpVolumeRatio {
    /// Creates a new `UpVolumeRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            window: VecDeque::with_capacity(period),
            total_vol: Decimal::ZERO,
            up_vol: Decimal::ZERO,
        })
    }
}

impl Signal for UpVolumeRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let is_up = bar.is_bullish();
        self.window.push_back((bar.volume, is_up));
        self.total_vol += bar.volume;
        if is_up { self.up_vol += bar.volume; }

        if self.window.len() > self.period {
            if let Some((ov, ou)) = self.window.pop_front() {
                self.total_vol -= ov;
                if ou { self.up_vol -= ov; }
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.total_vol.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar(self.up_vol / self.total_vol))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.total_vol = Decimal::ZERO; self.up_vol = Decimal::ZERO; }
    fn name(&self) -> &str { "UpVolumeRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str, v: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: dec!(200),
            low: dec!(1),
            close: c.parse().unwrap(),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_up_volume_ratio_all_up() {
        let mut sig = UpVolumeRatio::new(2).unwrap();
        sig.update(&bar("100", "110", "1000")).unwrap();
        let v = sig.update(&bar("100", "110", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_up_volume_ratio_mixed() {
        let mut sig = UpVolumeRatio::new(2).unwrap();
        sig.update(&bar("100", "110", "1000")).unwrap(); // up 1000
        let v = sig.update(&bar("110", "100", "1000")).unwrap(); // down 1000
        // up_vol = 1000, total = 2000, ratio = 0.5
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }
}
