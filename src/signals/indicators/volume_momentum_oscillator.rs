//! Volume Momentum Oscillator indicator.
//!
//! Computes the difference between a fast EMA of volume and a slow EMA of
//! volume, analogous to MACD but applied to volume rather than price. Measures
//! whether volume is accelerating (positive) or decelerating (negative).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Difference between fast-EMA(volume) and slow-EMA(volume).
///
/// ```text
/// vmo = EMA(volume, fast_period) - EMA(volume, slow_period)
/// ```
///
/// Positive values indicate volume is expanding relative to its longer-term
/// average (acceleration). Negative values indicate volume is contracting
/// (deceleration). Useful for confirming breakouts or identifying exhaustion.
///
/// Returns a value after the first bar (both EMAs seed on the first bar).
/// `period()` returns `slow_period`.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `fast_period == 0`, `slow_period == 0`,
/// or `fast_period >= slow_period`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeMomentumOscillator;
/// use fin_primitives::signals::Signal;
///
/// let vmo = VolumeMomentumOscillator::new("vmo", 5, 20).unwrap();
/// assert_eq!(vmo.period(), 20);
/// ```
pub struct VolumeMomentumOscillator {
    name: String,
    slow_period: usize,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    fast_k: Decimal,
    slow_k: Decimal,
}

impl VolumeMomentumOscillator {
    /// Constructs a new `VolumeMomentumOscillator`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if periods are zero or `fast >= slow`.
    pub fn new(
        name: impl Into<String>,
        fast_period: usize,
        slow_period: usize,
    ) -> Result<Self, FinError> {
        if fast_period == 0 {
            return Err(FinError::InvalidPeriod(fast_period));
        }
        if slow_period == 0 || fast_period >= slow_period {
            return Err(FinError::InvalidPeriod(slow_period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let fast_k = Decimal::from(2u32) / (Decimal::from(fast_period as u32) + Decimal::ONE);
        #[allow(clippy::cast_possible_truncation)]
        let slow_k = Decimal::from(2u32) / (Decimal::from(slow_period as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            slow_period,
            fast_ema: None,
            slow_ema: None,
            fast_k,
            slow_k,
        })
    }
}

impl crate::signals::Signal for VolumeMomentumOscillator {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.slow_period
    }

    fn is_ready(&self) -> bool {
        self.fast_ema.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;

        let fast = match self.fast_ema {
            None => { self.fast_ema = Some(vol); vol }
            Some(prev) => {
                let next = vol * self.fast_k + prev * (Decimal::ONE - self.fast_k);
                self.fast_ema = Some(next);
                next
            }
        };

        let slow = match self.slow_ema {
            None => { self.slow_ema = Some(vol); vol }
            Some(prev) => {
                let next = vol * self.slow_k + prev * (Decimal::ONE - self.slow_k);
                self.slow_ema = Some(next);
                next
            }
        };

        Ok(SignalValue::Scalar(fast - slow))
    }

    fn reset(&mut self) {
        self.fast_ema = None;
        self.slow_ema = None;
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
    fn test_vmo_invalid_period() {
        assert!(VolumeMomentumOscillator::new("vmo", 0, 20).is_err());
        assert!(VolumeMomentumOscillator::new("vmo", 5, 0).is_err());
        assert!(VolumeMomentumOscillator::new("vmo", 20, 5).is_err());
        assert!(VolumeMomentumOscillator::new("vmo", 5, 5).is_err());
    }

    #[test]
    fn test_vmo_ready_after_first_bar() {
        let mut vmo = VolumeMomentumOscillator::new("vmo", 5, 20).unwrap();
        vmo.update_bar(&bar("1000")).unwrap();
        assert!(vmo.is_ready());
    }

    #[test]
    fn test_vmo_constant_volume_zero() {
        // Both EMAs converge to the same value → difference = 0
        let mut vmo = VolumeMomentumOscillator::new("vmo", 5, 20).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..100 {
            last = vmo.update_bar(&bar("1000")).unwrap();
        }
        if let SignalValue::Scalar(s) = last {
            assert!(s.abs() < dec!(0.0001), "constant volume → VMO near 0: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vmo_rising_volume_positive() {
        let mut vmo = VolumeMomentumOscillator::new("vmo", 3, 10).unwrap();
        // Seed with low volume then spike
        for _ in 0..10 {
            vmo.update_bar(&bar("100")).unwrap();
        }
        let v = vmo.update_bar(&bar("10000")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0), "high volume spike → fast > slow → positive: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vmo_reset() {
        let mut vmo = VolumeMomentumOscillator::new("vmo", 5, 20).unwrap();
        vmo.update_bar(&bar("1000")).unwrap();
        assert!(vmo.is_ready());
        vmo.reset();
        assert!(!vmo.is_ready());
    }
}
