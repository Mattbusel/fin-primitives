//! Normalized Volume indicator — volume relative to its moving average.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Normalized Volume — divides the current bar's volume by the SMA of volume
/// over the last `period` bars.
///
/// A value of `1.0` means volume equals its average. Values above `1.0` indicate
/// above-average volume; values below `1.0` indicate below-average volume.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or when the
/// average volume is zero (e.g. all bars have zero volume).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NormalizedVolume;
/// use fin_primitives::signals::Signal;
/// let nv = NormalizedVolume::new("nvol_20", 20).unwrap();
/// assert_eq!(nv.period(), 20);
/// ```
pub struct NormalizedVolume {
    name: String,
    period: usize,
    volumes: VecDeque<Decimal>,
}

impl NormalizedVolume {
    /// Constructs a new `NormalizedVolume`.
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

impl Signal for NormalizedVolume {
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

        #[allow(clippy::cast_possible_truncation)]
        let period_d = Decimal::from(self.period as u32);
        let sum: Decimal = self.volumes.iter().copied().sum();
        let avg = sum
            .checked_div(period_d)
            .ok_or(FinError::ArithmeticOverflow)?;

        if avg.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = bar
            .volume
            .checked_div(avg)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
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

    fn bar_with_vol(vol: &str) -> OhlcvBar {
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
    fn test_nvol_invalid_period() {
        assert!(NormalizedVolume::new("nv", 0).is_err());
    }

    #[test]
    fn test_nvol_unavailable_before_period() {
        let mut nv = NormalizedVolume::new("nv", 3).unwrap();
        assert_eq!(nv.update_bar(&bar_with_vol("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(nv.update_bar(&bar_with_vol("200")).unwrap(), SignalValue::Unavailable);
        assert!(!nv.is_ready());
    }

    #[test]
    fn test_nvol_equal_volumes_gives_one() {
        let mut nv = NormalizedVolume::new("nv", 3).unwrap();
        nv.update_bar(&bar_with_vol("100")).unwrap();
        nv.update_bar(&bar_with_vol("100")).unwrap();
        let v = nv.update_bar(&bar_with_vol("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_nvol_double_avg_gives_two() {
        let mut nv = NormalizedVolume::new("nv", 3).unwrap();
        // avg = (100 + 100 + 200) / 3 = 133.333...; last = 200; 200/133.333 = 1.5
        // Let's use uniform base then a big bar.
        nv.update_bar(&bar_with_vol("100")).unwrap();
        nv.update_bar(&bar_with_vol("100")).unwrap();
        nv.update_bar(&bar_with_vol("100")).unwrap();
        // Now push 200 — window slides to [100, 100, 200], avg=133.33, ratio=1.5
        let v = nv.update_bar(&bar_with_vol("200")).unwrap();
        if let SignalValue::Scalar(ratio) = v {
            // 200 / ((100+100+200)/3) = 200 / 133.33... = 1.5
            assert!((ratio - dec!(1.5)).abs() < dec!(0.001), "expected 1.5, got {ratio}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_nvol_zero_average_returns_unavailable() {
        let mut nv = NormalizedVolume::new("nv", 2).unwrap();
        nv.update_bar(&bar_with_vol("0")).unwrap();
        let v = nv.update_bar(&bar_with_vol("0")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_nvol_reset() {
        let mut nv = NormalizedVolume::new("nv", 2).unwrap();
        nv.update_bar(&bar_with_vol("100")).unwrap();
        nv.update_bar(&bar_with_vol("100")).unwrap();
        assert!(nv.is_ready());
        nv.reset();
        assert!(!nv.is_ready());
    }

    #[test]
    fn test_nvol_period_and_name() {
        let nv = NormalizedVolume::new("my_nv", 20).unwrap();
        assert_eq!(nv.period(), 20);
        assert_eq!(nv.name(), "my_nv");
    }
}
