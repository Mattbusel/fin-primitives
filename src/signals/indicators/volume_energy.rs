//! Volume Energy indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Energy — rolling sum of `volume × |close - prev_close|` over the last
/// `period` bars.
///
/// Combines price velocity and volume to measure the kinetic energy of market
/// movement. High values indicate active, directional trading activity.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeEnergy;
/// use fin_primitives::signals::Signal;
///
/// let ve = VolumeEnergy::new("ve", 10).unwrap();
/// assert_eq!(ve.period(), 10);
/// ```
pub struct VolumeEnergy {
    name: String,
    period: usize,
    energies: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl VolumeEnergy {
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            energies: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for VolumeEnergy {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.energies.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let energy = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(pc) => bar.volume * (bar.close - pc).abs(),
        };
        self.prev_close = Some(bar.close);
        self.energies.push_back(energy);
        if self.energies.len() > self.period { self.energies.pop_front(); }
        if self.energies.len() < self.period { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(self.energies.iter().sum()))
    }

    fn reset(&mut self) {
        self.energies.clear();
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str, v: &str) -> OhlcvBar {
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let vq = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: cp, low: cp, close: cp,
            volume: vq,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ve_invalid() { assert!(VolumeEnergy::new("v", 0).is_err()); }

    #[test]
    fn test_ve_unavailable() {
        let mut ve = VolumeEnergy::new("v", 3).unwrap();
        // bar 1: no prev_close
        assert_eq!(ve.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
        // bar 2: 1 energy value, need 3
        assert_eq!(ve.update_bar(&bar("105", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ve_flat_price_zero_energy() {
        let mut ve = VolumeEnergy::new("v", 3).unwrap();
        ve.update_bar(&bar("100", "1000")).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 { last = ve.update_bar(&bar("100", "1000")).unwrap(); }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ve_moving_price_positive_energy() {
        let mut ve = VolumeEnergy::new("v", 2).unwrap();
        ve.update_bar(&bar("100", "1000")).unwrap(); // seed
        ve.update_bar(&bar("105", "1000")).unwrap(); // energy = 1000*5=5000, not ready
        let last = ve.update_bar(&bar("110", "1000")).unwrap(); // energy = 5000+5000=10000
        assert_eq!(last, SignalValue::Scalar(dec!(10000)));
    }

    #[test]
    fn test_ve_reset() {
        let mut ve = VolumeEnergy::new("v", 2).unwrap();
        ve.update_bar(&bar("100", "1000")).unwrap();
        ve.update_bar(&bar("105", "1000")).unwrap();
        ve.update_bar(&bar("110", "1000")).unwrap();
        assert!(ve.is_ready());
        ve.reset();
        assert!(!ve.is_ready());
    }
}
