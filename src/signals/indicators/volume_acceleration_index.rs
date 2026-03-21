//! Volume Acceleration Index indicator.
//!
//! Computes the difference between the current `period`-bar volume SMA and
//! the SMA computed `period` bars ago, measuring whether average volume is
//! growing (acceleration) or shrinking (deceleration).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Current SMA(volume, period) minus the SMA(volume, period) from `period` bars ago.
///
/// Positive values indicate trading activity is expanding over time; negative
/// values indicate it is contracting. A value of zero means volume activity
/// is stable.
///
/// Returns [`SignalValue::Unavailable`] until `2 × period` bars have accumulated
/// (needs two full SMA windows to compare).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeAccelerationIndex;
/// use fin_primitives::signals::Signal;
///
/// let vai = VolumeAccelerationIndex::new("vai", 10).unwrap();
/// assert_eq!(vai.period(), 10);
/// assert!(!vai.is_ready());
/// ```
pub struct VolumeAccelerationIndex {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,   // rolling 2*period volumes
    sma_history: VecDeque<Decimal>, // history of SMAs
    window_sum: Decimal,
}

impl VolumeAccelerationIndex {
    /// Constructs a new `VolumeAccelerationIndex`.
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
            window: VecDeque::with_capacity(period),
            sma_history: VecDeque::with_capacity(period + 1),
            window_sum: Decimal::ZERO,
        })
    }
}

impl crate::signals::Signal for VolumeAccelerationIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.sma_history.len() > self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;

        self.window_sum += vol;
        self.window.push_back(vol);

        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.window_sum -= old;
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        #[allow(clippy::cast_possible_truncation)]
        let current_sma = self.window_sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        self.sma_history.push_back(current_sma);
        if self.sma_history.len() > self.period + 1 {
            self.sma_history.pop_front();
        }

        if self.sma_history.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        let prior_sma = self.sma_history[0];
        Ok(SignalValue::Scalar(current_sma - prior_sma))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sma_history.clear();
        self.window_sum = Decimal::ZERO;
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
        let p = Price::new("100".parse().unwrap()).unwrap();
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
    fn test_vai_invalid_period() {
        assert!(VolumeAccelerationIndex::new("vai", 0).is_err());
    }

    #[test]
    fn test_vai_unavailable_during_warmup() {
        let mut vai = VolumeAccelerationIndex::new("vai", 3).unwrap();
        for _ in 0..5 {
            assert_eq!(vai.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vai_constant_volume_zero() {
        let mut vai = VolumeAccelerationIndex::new("vai", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..8 {
            last = vai.update_bar(&bar("1000")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vai_rising_volume_positive() {
        let mut vai = VolumeAccelerationIndex::new("vai", 2).unwrap();
        // Period 1: [100, 100] → SMA=100
        vai.update_bar(&bar("100")).unwrap();
        vai.update_bar(&bar("100")).unwrap();
        // Period 2: [200, 200] → SMA=200, prior=100, diff=100
        vai.update_bar(&bar("200")).unwrap();
        let v = vai.update_bar(&bar("200")).unwrap();
        // Now sma_history has [100, 200, 200]; prior=100, current=200, diff=100
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0), "rising volume → positive VAI: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vai_reset() {
        let mut vai = VolumeAccelerationIndex::new("vai", 3).unwrap();
        for _ in 0..8 {
            vai.update_bar(&bar("1000")).unwrap();
        }
        assert!(vai.is_ready());
        vai.reset();
        assert!(!vai.is_ready());
    }
}
