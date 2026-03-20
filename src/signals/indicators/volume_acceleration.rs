//! Volume Acceleration indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Acceleration — rate of change of volume EMA (second derivative of volume).
///
/// ```text
/// vol_ema_t     = EMA(volume, period)
/// vol_ema_{t−1} = previous EMA value
/// output        = (vol_ema_t − vol_ema_{t−1}) / vol_ema_{t−1} × 100
/// ```
///
/// Positive output indicates accelerating volume; negative decelerating.
/// Returns 0 when the previous EMA is zero.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` EMA values exist.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeAcceleration;
/// use fin_primitives::signals::Signal;
///
/// let va = VolumeAcceleration::new("va", 10).unwrap();
/// assert_eq!(va.period(), 10);
/// ```
pub struct VolumeAcceleration {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    prev_ema: Option<Decimal>,
    seed: Vec<Decimal>,
}

impl VolumeAcceleration {
    /// Creates a new `VolumeAcceleration`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            ema: None,
            prev_ema: None,
            seed: Vec::with_capacity(period),
        })
    }
}

impl Signal for VolumeAcceleration {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let k = Decimal::from(2u32) / Decimal::from((self.period + 1) as u32);

        if self.ema.is_none() {
            self.seed.push(bar.volume);
            if self.seed.len() == self.period {
                let sma = self.seed.iter().sum::<Decimal>()
                    / Decimal::from(self.period as u32);
                self.ema = Some(sma);
            }
            return Ok(SignalValue::Unavailable);
        }

        self.prev_ema = self.ema;
        let new_ema = self.ema.unwrap() + k * (bar.volume - self.ema.unwrap());
        self.ema = Some(new_ema);

        match self.prev_ema {
            None => Ok(SignalValue::Unavailable),
            Some(pe) if pe.is_zero() => Ok(SignalValue::Scalar(Decimal::ZERO)),
            Some(pe) => {
                let accel = (new_ema - pe) / pe * Decimal::from(100u32);
                Ok(SignalValue::Scalar(accel))
            }
        }
    }

    fn is_ready(&self) -> bool { self.prev_ema.is_some() }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.ema = None;
        self.prev_ema = None;
        self.seed.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_v(v: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
        let vol = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: vol,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_va_invalid() {
        assert!(VolumeAcceleration::new("v", 0).is_err());
    }

    #[test]
    fn test_va_unavailable_before_warmup() {
        let mut v = VolumeAcceleration::new("v", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(v.update_bar(&bar_v("1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_va_constant_volume_is_zero() {
        // Constant volume: EMA doesn't change → acceleration = 0
        let mut v = VolumeAcceleration::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 { last = v.update_bar(&bar_v("1000")).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_va_rising_volume_positive() {
        // Increasing volume: EMA rises → positive acceleration
        let mut v = VolumeAcceleration::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 { v.update_bar(&bar_v("1000")).unwrap(); }
        for _ in 0..5 { last = v.update_bar(&bar_v("5000")).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            assert!(val > dec!(0), "expected positive acceleration, got {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_va_reset() {
        let mut v = VolumeAcceleration::new("v", 3).unwrap();
        for _ in 0..10 { v.update_bar(&bar_v("1000")).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
