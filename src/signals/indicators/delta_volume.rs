//! Delta Volume indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Delta Volume — bar-to-bar absolute change in traded volume.
///
/// ```text
/// delta_volume[t] = volume[t] − volume[t−1]
/// ```
///
/// Positive values indicate increasing volume; negative values indicate
/// decreasing volume relative to the prior bar.
///
/// This is distinct from [`crate::signals::indicators::Vroc`] (which expresses
/// the change as a *percentage*) and from [`crate::signals::indicators::VolumeAcceleration`]
/// (which uses EMA smoothing before differencing).
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior volume exists).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DeltaVolume;
/// use fin_primitives::signals::Signal;
///
/// let dv = DeltaVolume::new("dv").unwrap();
/// assert_eq!(dv.period(), 1);
/// assert!(!dv.is_ready());
/// ```
pub struct DeltaVolume {
    name: String,
    prev_volume: Option<Decimal>,
}

impl DeltaVolume {
    /// Constructs a new `DeltaVolume`.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_volume: None })
    }
}

impl Signal for DeltaVolume {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_volume {
            None => SignalValue::Unavailable,
            Some(prev) => SignalValue::Scalar(bar.volume - prev),
        };
        self.prev_volume = Some(bar.volume);
        Ok(result)
    }

    fn is_ready(&self) -> bool {
        self.prev_volume.is_some()
    }

    fn period(&self) -> usize {
        1
    }

    fn reset(&mut self) {
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

    fn bar(close: &str, volume: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        let v = Quantity::new(volume.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_delta_volume_first_bar_unavailable() {
        let mut dv = DeltaVolume::new("dv").unwrap();
        assert_eq!(dv.update_bar(&bar("100", "500")).unwrap(), SignalValue::Unavailable);
        assert!(dv.is_ready()); // ready means we have a prev value now
    }

    #[test]
    fn test_delta_volume_positive_increase() {
        let mut dv = DeltaVolume::new("dv").unwrap();
        dv.update_bar(&bar("100", "500")).unwrap();
        let v = dv.update_bar(&bar("101", "800")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(300)));
    }

    #[test]
    fn test_delta_volume_negative_decrease() {
        let mut dv = DeltaVolume::new("dv").unwrap();
        dv.update_bar(&bar("100", "800")).unwrap();
        let v = dv.update_bar(&bar("99", "500")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-300)));
    }

    #[test]
    fn test_delta_volume_zero_change() {
        let mut dv = DeltaVolume::new("dv").unwrap();
        dv.update_bar(&bar("100", "500")).unwrap();
        let v = dv.update_bar(&bar("100", "500")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_delta_volume_reset() {
        let mut dv = DeltaVolume::new("dv").unwrap();
        dv.update_bar(&bar("100", "500")).unwrap();
        assert!(dv.is_ready());
        dv.reset();
        assert!(!dv.is_ready());
        assert_eq!(dv.update_bar(&bar("100", "500")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_delta_volume_period_is_1() {
        let dv = DeltaVolume::new("dv").unwrap();
        assert_eq!(dv.period(), 1);
    }
}
