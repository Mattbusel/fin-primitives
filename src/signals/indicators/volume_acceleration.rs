//! Volume Acceleration indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Acceleration — the rate of change of volume over `period` bars.
///
/// ```text
/// volume_acceleration = (current_volume - volume[period_bars_ago]) / volume[period_bars_ago] * 100
/// ```
///
/// A positive value indicates volume is growing; negative indicates volume is shrinking.
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen,
/// or if the reference volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeAcceleration;
/// use fin_primitives::signals::Signal;
///
/// let va = VolumeAcceleration::new("va", 5).unwrap();
/// assert_eq!(va.period(), 5);
/// ```
pub struct VolumeAcceleration {
    name: String,
    period: usize,
    volumes: VecDeque<Decimal>,
}

impl VolumeAcceleration {
    /// Constructs a new `VolumeAcceleration`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            volumes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for VolumeAcceleration {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.volumes.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.volumes.push_back(bar.volume);
        if self.volumes.len() > self.period + 1 {
            self.volumes.pop_front();
        }
        if self.volumes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }
        let old_vol = self.volumes[0];
        let new_vol = *self.volumes.back().unwrap();
        if old_vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let acc = (new_vol - old_vol) / old_vol * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(acc))
    }

    fn reset(&mut self) {
        self.volumes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(v: &str) -> OhlcvBar {
        let p = Price::new("100".parse().unwrap()).unwrap();
        let vq = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: vq,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_va_invalid_period() {
        assert!(VolumeAcceleration::new("va", 0).is_err());
    }

    #[test]
    fn test_va_unavailable_before_warm_up() {
        let mut va = VolumeAcceleration::new("va", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(va.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_va_doubled_volume() {
        let mut va = VolumeAcceleration::new("va", 2).unwrap();
        va.update_bar(&bar("1000")).unwrap();
        va.update_bar(&bar("1000")).unwrap();
        // old=1000, new=2000 → +100%
        let result = va.update_bar(&bar("2000")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_va_halved_volume() {
        let mut va = VolumeAcceleration::new("va", 2).unwrap();
        va.update_bar(&bar("2000")).unwrap();
        va.update_bar(&bar("2000")).unwrap();
        // old=2000, new=1000 → -50%
        let result = va.update_bar(&bar("1000")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(-50)));
    }

    #[test]
    fn test_va_reset() {
        let mut va = VolumeAcceleration::new("va", 2).unwrap();
        for _ in 0..3 { va.update_bar(&bar("1000")).unwrap(); }
        assert!(va.is_ready());
        va.reset();
        assert!(!va.is_ready());
    }
}
