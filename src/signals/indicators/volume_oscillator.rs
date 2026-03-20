//! Volume Oscillator indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume Oscillator (VO) — measures the difference between two volume EMAs as a percentage.
///
/// ```text
/// VO = (EMA(volume, fast) - EMA(volume, slow)) / EMA(volume, slow) * 100
/// ```
///
/// Positive values indicate increasing volume momentum (fast EMA above slow EMA).
/// Negative values indicate decreasing volume momentum.
/// Returns [`SignalValue::Unavailable`] until the slow EMA has been seeded
/// (i.e., `slow_period` bars have been seen).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeOscillator;
/// use fin_primitives::signals::Signal;
///
/// let vo = VolumeOscillator::new("vo", 5, 14).unwrap();
/// assert_eq!(vo.period(), 14);
/// ```
pub struct VolumeOscillator {
    name: String,
    fast_period: usize,
    slow_period: usize,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    fast_alpha: Decimal,
    slow_alpha: Decimal,
    bars: usize,
}

impl VolumeOscillator {
    /// Constructs a new `VolumeOscillator`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero, or if `fast >= slow`.
    pub fn new(
        name: impl Into<String>,
        fast_period: usize,
        slow_period: usize,
    ) -> Result<Self, FinError> {
        if fast_period == 0 || slow_period == 0 {
            return Err(FinError::InvalidPeriod(fast_period.min(slow_period)));
        }
        if fast_period >= slow_period {
            return Err(FinError::InvalidPeriod(fast_period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let fast_alpha = Decimal::TWO / Decimal::from((fast_period + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let slow_alpha = Decimal::TWO / Decimal::from((slow_period + 1) as u32);
        Ok(Self {
            name: name.into(),
            fast_period,
            slow_period,
            fast_ema: None,
            slow_ema: None,
            fast_alpha,
            slow_alpha,
            bars: 0,
        })
    }

    fn ema_step(prev: Option<Decimal>, alpha: Decimal, value: Decimal) -> Decimal {
        match prev {
            None => value,
            Some(p) => p + alpha * (value - p),
        }
    }
}

impl Signal for VolumeOscillator {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.slow_period
    }

    fn is_ready(&self) -> bool {
        self.bars >= self.slow_period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;
        self.bars += 1;
        self.fast_ema = Some(Self::ema_step(self.fast_ema, self.fast_alpha, vol));
        self.slow_ema = Some(Self::ema_step(self.slow_ema, self.slow_alpha, vol));

        if self.bars < self.slow_period {
            return Ok(SignalValue::Unavailable);
        }

        let fast = self.fast_ema.unwrap();
        let slow = self.slow_ema.unwrap();

        if slow.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let vo = (fast - slow)
            .checked_div(slow)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::from(100u32);

        Ok(SignalValue::Scalar(vo))
    }

    fn reset(&mut self) {
        self.fast_ema = None;
        self.slow_ema = None;
        self.bars = 0;
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
    fn test_vo_invalid_periods() {
        assert!(VolumeOscillator::new("vo", 0, 14).is_err());
        assert!(VolumeOscillator::new("vo", 5, 0).is_err());
        assert!(VolumeOscillator::new("vo", 14, 5).is_err()); // fast >= slow
        assert!(VolumeOscillator::new("vo", 5, 5).is_err());  // fast == slow
    }

    #[test]
    fn test_vo_unavailable_before_slow_period() {
        let mut vo = VolumeOscillator::new("vo", 2, 5).unwrap();
        for _ in 0..4 {
            assert_eq!(vo.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!vo.is_ready());
    }

    #[test]
    fn test_vo_constant_volume_near_zero() {
        let mut vo = VolumeOscillator::new("vo", 2, 5).unwrap();
        for _ in 0..5 {
            vo.update_bar(&bar("1000")).unwrap();
        }
        assert!(vo.is_ready());
        if let SignalValue::Scalar(v) = vo.update_bar(&bar("1000")).unwrap() {
            // Both EMAs converge to 1000 => VO approaches 0
            assert!(v.abs() < dec!(1), "expected near-zero VO for constant volume, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vo_volume_surge_positive() {
        let mut vo = VolumeOscillator::new("vo", 2, 5).unwrap();
        // Seed with normal volume
        for _ in 0..5 {
            vo.update_bar(&bar("1000")).unwrap();
        }
        // Volume surge: fast EMA should jump faster than slow
        let v = vo.update_bar(&bar("10000")).unwrap();
        if let SignalValue::Scalar(osc) = v {
            assert!(osc > dec!(0), "expected positive VO on volume surge, got {osc}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vo_reset() {
        let mut vo = VolumeOscillator::new("vo", 2, 5).unwrap();
        for _ in 0..5 {
            vo.update_bar(&bar("1000")).unwrap();
        }
        assert!(vo.is_ready());
        vo.reset();
        assert!(!vo.is_ready());
    }
}
