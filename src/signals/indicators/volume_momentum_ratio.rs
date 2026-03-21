//! Volume Momentum Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Momentum Ratio — ratio of the recent short-period average volume to
/// the longer-period average volume, measuring volume acceleration.
///
/// ```text
/// fast_avg_vol = mean(volume[t-fast+1 .. t])
/// slow_avg_vol = mean(volume[t-slow+1 .. t])
/// vmr          = fast_avg_vol / slow_avg_vol
/// ```
///
/// - **> 1.0**: recent volume exceeds its longer-term average — participation increasing.
/// - **< 1.0**: recent volume is below average — participation declining.
/// - **≈ 1.0**: consistent volume participation.
///
/// Returns [`SignalValue::Unavailable`] until `slow` bars have been seen, or when
/// the slow average is zero.
///
/// # Errors
/// Returns [`FinError::InvalidInput`] if `fast >= slow` or `fast == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeMomentumRatio;
/// use fin_primitives::signals::Signal;
/// let vmr = VolumeMomentumRatio::new("vmr", 5, 20).unwrap();
/// assert_eq!(vmr.period(), 20);
/// ```
pub struct VolumeMomentumRatio {
    name: String,
    fast: usize,
    slow: usize,
    volumes: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeMomentumRatio {
    /// Constructs a new `VolumeMomentumRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `fast == 0` or `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 || fast >= slow {
            return Err(FinError::InvalidInput("fast must be > 0 and < slow".into()));
        }
        Ok(Self {
            name: name.into(),
            fast,
            slow,
            volumes: VecDeque::with_capacity(slow),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeMomentumRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.slow }
    fn is_ready(&self) -> bool { self.volumes.len() >= self.slow }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let v = bar.volume;
        self.sum += v;
        self.volumes.push_back(v);
        if self.volumes.len() > self.slow {
            let removed = self.volumes.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.volumes.len() < self.slow {
            return Ok(SignalValue::Unavailable);
        }

        let slow_avg = self.sum
            .checked_div(Decimal::from(self.slow as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if slow_avg.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        // Fast average: last `fast` volumes
        let fast_sum: Decimal = self.volumes.iter().rev().take(self.fast).copied().sum();
        let fast_avg = fast_sum
            .checked_div(Decimal::from(self.fast as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let ratio = fast_avg
            .checked_div(slow_avg)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.volumes.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(vol: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vmr_invalid_params() {
        assert!(VolumeMomentumRatio::new("v", 0, 20).is_err());
        assert!(VolumeMomentumRatio::new("v", 20, 20).is_err());
        assert!(VolumeMomentumRatio::new("v", 25, 20).is_err());
    }

    #[test]
    fn test_vmr_unavailable_during_warmup() {
        let mut vmr = VolumeMomentumRatio::new("v", 2, 5).unwrap();
        for v in &["100", "200", "300", "400"] {
            assert_eq!(vmr.update_bar(&bar(v)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!vmr.is_ready());
    }

    #[test]
    fn test_vmr_uniform_volume_one() {
        // Same volume every bar → fast_avg = slow_avg → ratio = 1
        let mut vmr = VolumeMomentumRatio::new("v", 2, 4).unwrap();
        for _ in 0..5 {
            vmr.update_bar(&bar("1000")).unwrap();
        }
        if let SignalValue::Scalar(v) = vmr.update_bar(&bar("1000")).unwrap() {
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vmr_spike_above_one() {
        // Low volume baseline then recent spike → ratio > 1
        let mut vmr = VolumeMomentumRatio::new("v", 2, 4).unwrap();
        vmr.update_bar(&bar("100")).unwrap();
        vmr.update_bar(&bar("100")).unwrap();
        vmr.update_bar(&bar("100")).unwrap();
        vmr.update_bar(&bar("100")).unwrap();
        // Spike: last 2 bars = 1000
        vmr.update_bar(&bar("1000")).unwrap();
        if let SignalValue::Scalar(v) = vmr.update_bar(&bar("1000")).unwrap() {
            assert!(v > dec!(1), "recent spike → VMR > 1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vmr_reset() {
        let mut vmr = VolumeMomentumRatio::new("v", 2, 4).unwrap();
        for _ in 0..4 { vmr.update_bar(&bar("100")).unwrap(); }
        assert!(vmr.is_ready());
        vmr.reset();
        assert!(!vmr.is_ready());
    }
}
