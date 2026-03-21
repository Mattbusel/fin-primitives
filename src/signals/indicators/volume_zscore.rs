//! Volume Z-Score indicator.
//!
//! Measures how many standard deviations the current bar's volume deviates
//! from its rolling mean — a normalized measure of volume activity.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Z-Score of volume: `(volume - mean(volume, N)) / std(volume, N)`.
///
/// Positive values indicate above-average volume; negative values indicate
/// below-average volume. Returns zero when the rolling standard deviation is
/// zero (i.e. all volumes in the window are identical).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeZScore;
/// use fin_primitives::signals::Signal;
///
/// let vz = VolumeZScore::new("vol_z", 20).unwrap();
/// assert_eq!(vz.period(), 20);
/// assert!(!vz.is_ready());
/// ```
pub struct VolumeZScore {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl VolumeZScore {
    /// Constructs a new `VolumeZScore` with the given name and period.
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
        })
    }
}

impl Signal for VolumeZScore {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;

        self.window.push_back(vol);
        if self.window.len() > self.period {
            self.window.pop_front();
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        #[allow(clippy::cast_possible_truncation)]
        let n = Decimal::from(self.period as u32);
        let mean = self.window.iter().copied().sum::<Decimal>()
            .checked_div(n)
            .ok_or(FinError::ArithmeticOverflow)?;

        let variance = self
            .window
            .iter()
            .map(|&v| {
                let diff = v - mean;
                diff * diff
            })
            .sum::<Decimal>()
            .checked_div(n)
            .ok_or(FinError::ArithmeticOverflow)?;

        use rust_decimal::prelude::ToPrimitive;
        let variance_f64 = variance.to_f64().unwrap_or(0.0);
        let std_dev = Decimal::try_from(variance_f64.sqrt()).unwrap_or(Decimal::ZERO);

        if std_dev.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let z = (vol - mean)
            .checked_div(std_dev)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(z))
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_vol(vol: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
        let v = vol.parse().unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::new(v).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_volume_zscore_invalid_period() {
        assert!(VolumeZScore::new("vz", 0).is_err());
    }

    #[test]
    fn test_volume_zscore_unavailable_during_warmup() {
        let mut vz = VolumeZScore::new("vz", 3).unwrap();
        assert_eq!(vz.update_bar(&bar_vol("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(vz.update_bar(&bar_vol("200")).unwrap(), SignalValue::Unavailable);
        assert!(!vz.is_ready());
    }

    #[test]
    fn test_volume_zscore_ready_after_period() {
        let mut vz = VolumeZScore::new("vz", 3).unwrap();
        vz.update_bar(&bar_vol("100")).unwrap();
        vz.update_bar(&bar_vol("200")).unwrap();
        vz.update_bar(&bar_vol("300")).unwrap();
        assert!(vz.is_ready());
    }

    #[test]
    fn test_volume_zscore_constant_volume_returns_zero() {
        let mut vz = VolumeZScore::new("vz", 3).unwrap();
        for _ in 0..3 {
            vz.update_bar(&bar_vol("100")).unwrap();
        }
        // 4th bar: window is [100, 100, 100], std = 0, z = 0
        let v = vz.update_bar(&bar_vol("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_volume_zscore_high_bar_positive() {
        let mut vz = VolumeZScore::new("vz", 4).unwrap();
        for _ in 0..4 {
            vz.update_bar(&bar_vol("100")).unwrap();
        }
        // Very high volume bar should give a strongly positive z-score
        let v = vz.update_bar(&bar_vol("1000")).unwrap();
        if let SignalValue::Scalar(z) = v {
            assert!(z > dec!(0), "high volume should give positive z: {z}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_volume_zscore_low_bar_negative() {
        let mut vz = VolumeZScore::new("vz", 4).unwrap();
        for _ in 0..4 {
            vz.update_bar(&bar_vol("1000")).unwrap();
        }
        // Very low volume bar should give a negative z-score
        let v = vz.update_bar(&bar_vol("1")).unwrap();
        if let SignalValue::Scalar(z) = v {
            assert!(z < dec!(0), "low volume should give negative z: {z}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_volume_zscore_reset() {
        let mut vz = VolumeZScore::new("vz", 3).unwrap();
        for _ in 0..3 {
            vz.update_bar(&bar_vol("100")).unwrap();
        }
        assert!(vz.is_ready());
        vz.reset();
        assert!(!vz.is_ready());
        assert_eq!(vz.update_bar(&bar_vol("100")).unwrap(), SignalValue::Unavailable);
    }
}
