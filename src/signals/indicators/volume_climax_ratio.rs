//! Volume Climax Ratio — current volume as a fraction of its N-period maximum.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Climax Ratio — `volume / max(volume over last period bars)`.
///
/// A value of `1.0` means the current bar has the highest volume seen in the window.
/// Values approaching `1.0` indicate a potential climax move. Values near `0` indicate
/// light volume relative to the recent maximum.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or when
/// the maximum volume in the window is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeClimaxRatio;
/// use fin_primitives::signals::Signal;
/// let vcr = VolumeClimaxRatio::new("vcr_20", 20).unwrap();
/// assert_eq!(vcr.period(), 20);
/// ```
pub struct VolumeClimaxRatio {
    name: String,
    period: usize,
    volumes: VecDeque<Decimal>,
}

impl VolumeClimaxRatio {
    /// Constructs a new `VolumeClimaxRatio`.
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
            volumes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolumeClimaxRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.volumes.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.volumes.push_back(bar.volume);
        if self.volumes.len() > self.period {
            self.volumes.pop_front();
        }
        if self.volumes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let max_vol = self.volumes.iter().copied().fold(Decimal::ZERO, Decimal::max);
        if max_vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = bar
            .volume
            .checked_div(max_vol)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio.min(Decimal::ONE)))
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

    fn bar_vol(vol: &str) -> OhlcvBar {
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
    fn test_vcr_invalid_period() {
        assert!(VolumeClimaxRatio::new("vcr", 0).is_err());
    }

    #[test]
    fn test_vcr_unavailable_before_period() {
        let mut vcr = VolumeClimaxRatio::new("vcr", 3).unwrap();
        assert_eq!(vcr.update_bar(&bar_vol("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(vcr.update_bar(&bar_vol("200")).unwrap(), SignalValue::Unavailable);
        assert!(!vcr.is_ready());
    }

    #[test]
    fn test_vcr_max_volume_gives_one() {
        let mut vcr = VolumeClimaxRatio::new("vcr", 3).unwrap();
        vcr.update_bar(&bar_vol("100")).unwrap();
        vcr.update_bar(&bar_vol("100")).unwrap();
        vcr.update_bar(&bar_vol("100")).unwrap();
        // Push a bar with 5x the max → ratio should cap at 1.0
        let v = vcr.update_bar(&bar_vol("500")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_vcr_low_volume_fraction() {
        let mut vcr = VolumeClimaxRatio::new("vcr", 3).unwrap();
        vcr.update_bar(&bar_vol("1000")).unwrap();
        vcr.update_bar(&bar_vol("1000")).unwrap();
        vcr.update_bar(&bar_vol("1000")).unwrap();
        // Current vol = 100, max = 1000 → ratio = 0.1
        let v = vcr.update_bar(&bar_vol("100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(0.1)).abs() < dec!(0.001), "expected 0.1, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vcr_zero_volume_unavailable() {
        let mut vcr = VolumeClimaxRatio::new("vcr", 2).unwrap();
        vcr.update_bar(&bar_vol("0")).unwrap();
        let v = vcr.update_bar(&bar_vol("0")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_vcr_output_in_unit_interval() {
        let mut vcr = VolumeClimaxRatio::new("vcr", 3).unwrap();
        let vols = ["100", "200", "150", "300", "50"];
        for v in &vols {
            if let SignalValue::Scalar(r) = vcr.update_bar(&bar_vol(v)).unwrap() {
                assert!(r >= dec!(0));
                assert!(r <= dec!(1));
            }
        }
    }

    #[test]
    fn test_vcr_reset() {
        let mut vcr = VolumeClimaxRatio::new("vcr", 2).unwrap();
        vcr.update_bar(&bar_vol("100")).unwrap();
        vcr.update_bar(&bar_vol("200")).unwrap();
        assert!(vcr.is_ready());
        vcr.reset();
        assert!(!vcr.is_ready());
    }

    #[test]
    fn test_vcr_period_and_name() {
        let vcr = VolumeClimaxRatio::new("my_vcr", 20).unwrap();
        assert_eq!(vcr.period(), 20);
        assert_eq!(vcr.name(), "my_vcr");
    }
}
