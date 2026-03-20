//! Volume Rate of Change (VROC) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Rate of Change (VROC) — the percentage change in volume over `period` bars.
///
/// ```text
/// VROC = (volume[now] − volume[period_ago]) / volume[period_ago] × 100
/// ```
///
/// Positive values indicate volume is expanding relative to `period` bars ago;
/// negative values indicate volume contraction.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (needs the comparison bar), or when `volume[period_ago]` is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Vroc;
/// use fin_primitives::signals::Signal;
///
/// let v = Vroc::new("vroc10", 10).unwrap();
/// assert_eq!(v.period(), 10);
/// ```
pub struct Vroc {
    name: String,
    period: usize,
    volumes: VecDeque<Decimal>,
}

impl Vroc {
    /// Constructs a new `Vroc`.
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

impl Signal for Vroc {
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
        let vol_old = *self.volumes.front().unwrap();
        if vol_old.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let vol_now = *self.volumes.back().unwrap();
        let vroc = (vol_now - vol_old)
            .checked_div(vol_old)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(vroc))
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

    fn bar(vol: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
        let q = Quantity::new(vol.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: q,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vroc_invalid_period() {
        assert!(Vroc::new("v", 0).is_err());
    }

    #[test]
    fn test_vroc_unavailable_before_ready() {
        let mut v = Vroc::new("v", 2).unwrap();
        assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!v.is_ready());
    }

    #[test]
    fn test_vroc_flat_volume_is_zero() {
        let mut v = Vroc::new("v", 2).unwrap();
        v.update_bar(&bar("100")).unwrap();
        v.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(val) = v.update_bar(&bar("100")).unwrap() {
            assert_eq!(val, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vroc_volume_doubles_is_100() {
        let mut v = Vroc::new("v", 1).unwrap();
        v.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(val) = v.update_bar(&bar("200")).unwrap() {
            assert_eq!(val, dec!(100), "volume doubled → VROC = 100%");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vroc_volume_halves_is_negative50() {
        let mut v = Vroc::new("v", 1).unwrap();
        v.update_bar(&bar("200")).unwrap();
        if let SignalValue::Scalar(val) = v.update_bar(&bar("100")).unwrap() {
            assert_eq!(val, dec!(-50), "volume halved → VROC = -50%");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vroc_zero_old_volume_unavailable() {
        let mut v = Vroc::new("v", 1).unwrap();
        v.update_bar(&bar("0")).unwrap();
        assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vroc_reset() {
        let mut v = Vroc::new("v", 1).unwrap();
        v.update_bar(&bar("100")).unwrap();
        v.update_bar(&bar("100")).unwrap();
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
        assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vroc_period() {
        let v = Vroc::new("v", 5).unwrap();
        assert_eq!(v.period(), 5);
    }
}
