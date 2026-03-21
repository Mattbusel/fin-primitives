//! Volume Surge Detector indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Surge Detector.
///
/// Detects when the current bar's volume significantly exceeds the rolling average,
/// returning the ratio of current volume to the average.
///
/// Formula: `surge = volume_t / mean(volume, period)`
///
/// - > `threshold`: significant volume surge (returned value is the ratio).
/// - 1.0: current volume equals the rolling average.
/// - < 1.0: below-average volume.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
/// Returns `SignalValue::Scalar(0.0)` when mean volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeSurgeDetector;
/// use fin_primitives::signals::Signal;
/// let vsd = VolumeSurgeDetector::new("vsd_20", 20).unwrap();
/// assert_eq!(vsd.period(), 20);
/// ```
pub struct VolumeSurgeDetector {
    name: String,
    period: usize,
    volumes: VecDeque<Decimal>,
}

impl VolumeSurgeDetector {
    /// Constructs a new `VolumeSurgeDetector`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, volumes: VecDeque::with_capacity(period) })
    }
}

impl Signal for VolumeSurgeDetector {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.volumes.push_back(bar.volume);
        if self.volumes.len() > self.period {
            self.volumes.pop_front();
        }
        if self.volumes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.volumes.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let mean = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if mean.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let surge = bar.volume.checked_div(mean).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(surge))
    }

    fn is_ready(&self) -> bool {
        self.volumes.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
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
    fn test_period_zero_fails() {
        assert!(matches!(VolumeSurgeDetector::new("vsd", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut vsd = VolumeSurgeDetector::new("vsd", 3).unwrap();
        assert_eq!(vsd.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_equal_volume_gives_one() {
        let mut vsd = VolumeSurgeDetector::new("vsd", 3).unwrap();
        for _ in 0..3 {
            vsd.update_bar(&bar("100")).unwrap();
        }
        let v = vsd.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_high_volume_surge_above_one() {
        // 3 bars at vol=100, then a bar at vol=300
        // window=[100,100,300], mean=166.67, surge=300/166.67 > 1
        let mut vsd = VolumeSurgeDetector::new("vsd", 3).unwrap();
        for _ in 0..3 {
            vsd.update_bar(&bar("100")).unwrap();
        }
        let v = vsd.update_bar(&bar("300")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(1));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_zero_volume_returns_zero() {
        let mut vsd = VolumeSurgeDetector::new("vsd", 3).unwrap();
        for _ in 0..3 {
            vsd.update_bar(&bar("0")).unwrap();
        }
        let v = vsd.update_bar(&bar("0")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut vsd = VolumeSurgeDetector::new("vsd", 2).unwrap();
        vsd.update_bar(&bar("100")).unwrap();
        vsd.update_bar(&bar("100")).unwrap();
        assert!(vsd.is_ready());
        vsd.reset();
        assert!(!vsd.is_ready());
    }
}
