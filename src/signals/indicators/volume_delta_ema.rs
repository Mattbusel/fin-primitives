//! Volume Delta EMA indicator.
//!
//! EMA of bar-to-bar volume change percentage, smoothing out single-bar spikes
//! to reveal sustained acceleration or deceleration in participation.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume Delta EMA — EMA of `(volume[i] - volume[i-1]) / volume[i-1] × 100`.
///
/// Each bar's raw volume delta is:
/// ```text
/// delta[i] = (volume[i] - volume[i-1]) / volume[i-1] × 100   when prev_vol > 0
///          = 0                                                when prev_vol == 0
/// ```
///
/// The EMA smooths this series:
/// - **Positive and rising**: volume is consistently expanding — increasing
///   market participation or breakout confirmation.
/// - **Negative and falling**: volume is persistently fading — weak move,
///   potential exhaustion.
/// - **Near 0**: volume is stable relative to recent history.
///
/// Returns a value from the second bar (EMA seeds with first delta).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeDeltaEma;
/// use fin_primitives::signals::Signal;
/// let vde = VolumeDeltaEma::new("vde_10", 10).unwrap();
/// assert_eq!(vde.period(), 10);
/// ```
pub struct VolumeDeltaEma {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
    prev_volume: Option<Decimal>,
}

impl VolumeDeltaEma {
    /// Constructs a new `VolumeDeltaEma`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::from(2u32) / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { name: name.into(), period, ema: None, k, prev_volume: None })
    }
}

impl Signal for VolumeDeltaEma {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ema.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;
        let pv = self.prev_volume;
        self.prev_volume = Some(vol);

        let Some(prev_vol) = pv else {
            return Ok(SignalValue::Unavailable);
        };

        let delta = if prev_vol.is_zero() {
            Decimal::ZERO
        } else {
            let hundred = Decimal::from(100u32);
            (vol - prev_vol)
                .checked_div(prev_vol)
                .ok_or(FinError::ArithmeticOverflow)?
                .checked_mul(hundred)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        let ema = match self.ema {
            None => {
                self.ema = Some(delta);
                delta
            }
            Some(prev) => {
                let next = delta * self.k + prev * (Decimal::ONE - self.k);
                self.ema = Some(next);
                next
            }
        };

        Ok(SignalValue::Scalar(ema))
    }

    fn reset(&mut self) {
        self.ema = None;
        self.prev_volume = None;
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
    fn test_vde_invalid_period() {
        assert!(VolumeDeltaEma::new("vde", 0).is_err());
    }

    #[test]
    fn test_vde_first_bar_unavailable() {
        let mut vde = VolumeDeltaEma::new("vde", 5).unwrap();
        assert_eq!(vde.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
        assert!(!vde.is_ready());
    }

    #[test]
    fn test_vde_ready_after_second_bar() {
        let mut vde = VolumeDeltaEma::new("vde", 5).unwrap();
        vde.update_bar(&bar("1000")).unwrap();
        vde.update_bar(&bar("1100")).unwrap();
        assert!(vde.is_ready());
    }

    #[test]
    fn test_vde_stable_volume_zero() {
        // Same volume every bar → delta = 0 → EMA → 0
        let mut vde = VolumeDeltaEma::new("vde", 5).unwrap();
        for _ in 0..10 { vde.update_bar(&bar("1000")).unwrap(); }
        if let SignalValue::Scalar(v) = vde.update_bar(&bar("1000")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vde_doubling_volume_positive() {
        // Volume doubles each bar → delta = 100% each → EMA seeds at 100
        let mut vde = VolumeDeltaEma::new("vde", 3).unwrap();
        vde.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = vde.update_bar(&bar("200")).unwrap() {
            // delta=100, first EMA = 100
            assert_eq!(v, dec!(100));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vde_halving_volume_negative() {
        let mut vde = VolumeDeltaEma::new("vde", 3).unwrap();
        vde.update_bar(&bar("1000")).unwrap();
        if let SignalValue::Scalar(v) = vde.update_bar(&bar("500")).unwrap() {
            // delta = (500-1000)/1000 * 100 = -50
            assert_eq!(v, dec!(-50));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vde_reset() {
        let mut vde = VolumeDeltaEma::new("vde", 5).unwrap();
        vde.update_bar(&bar("1000")).unwrap();
        vde.update_bar(&bar("1100")).unwrap();
        assert!(vde.is_ready());
        vde.reset();
        assert!(!vde.is_ready());
        assert_eq!(vde.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
    }
}
