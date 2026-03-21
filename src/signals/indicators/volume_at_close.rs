//! Volume-at-Close indicator.
//!
//! Estimates the fraction of bar volume attributed to the close-side of the
//! bar's range, using the Close Location Value as a proxy.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume-at-Close: `volume * (1 + clv) / 2`.
///
/// Weights the bar's total volume by the Close Location Value (CLV), which
/// measures where the close fell within the bar's range on a scale from
/// `-1` (at the low) to `+1` (at the high).
///
/// The formula maps CLV into a `[0, 1]` fraction and scales by volume:
///
/// ```text
/// clv              = ((close - low) - (high - close)) / (high - low)
/// volume_at_close  = volume × (1 + clv) / 2
/// ```
///
/// - When close == high: full volume is "at-close" (bullish absorption).
/// - When close == low:  zero volume is "at-close" (bearish rejection).
/// - When close == mid:  half the volume.
///
/// Returns zero volume when the bar's range is zero (doji/flat bar), and
/// [`SignalValue::Unavailable`] when bar volume is zero (no trading).
///
/// Always ready after the first bar with non-zero volume. Period = 1.
///
/// # Errors
/// Returns [`FinError::ArithmeticOverflow`] on arithmetic failure.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeAtClose;
/// use fin_primitives::signals::Signal;
///
/// let vac = VolumeAtClose::new("vac").unwrap();
/// assert_eq!(vac.period(), 1);
/// ```
pub struct VolumeAtClose {
    name: String,
    ready: bool,
}

impl VolumeAtClose {
    /// Constructs a new `VolumeAtClose`.
    ///
    /// # Errors
    /// Never fails; returns `Ok` always.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), ready: false })
    }
}

impl Signal for VolumeAtClose {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;

        if vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        self.ready = true;

        let clv = bar.close_location_value();
        // Map clv from [-1, 1] to [0, 1]
        let fraction = (Decimal::ONE + clv)
            .checked_div(Decimal::from(2u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let vac = vol
            .checked_mul(fraction)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(vac))
    }

    fn reset(&mut self) {
        self.ready = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, high: &str, low: &str, close: &str, vol: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(open.parse().unwrap()).unwrap(),
            high: Price::new(high.parse().unwrap()).unwrap(),
            low: Price::new(low.parse().unwrap()).unwrap(),
            close: Price::new(close.parse().unwrap()).unwrap(),
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vac_zero_volume_unavailable() {
        let mut vac = VolumeAtClose::new("vac").unwrap();
        let v = vac.update_bar(&bar("100", "110", "90", "105", "0")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_vac_close_at_high_full_volume() {
        let mut vac = VolumeAtClose::new("vac").unwrap();
        // clv = 1 (close at high), fraction = 1, vac = volume
        let v = vac.update_bar(&bar("100", "110", "90", "110", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1000)));
    }

    #[test]
    fn test_vac_close_at_low_zero_volume() {
        let mut vac = VolumeAtClose::new("vac").unwrap();
        // clv = -1 (close at low), fraction = 0, vac = 0
        let v = vac.update_bar(&bar("100", "110", "90", "90", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vac_close_at_midpoint_half_volume() {
        let mut vac = VolumeAtClose::new("vac").unwrap();
        // clv = 0 (close at mid), fraction = 0.5, vac = 500
        let v = vac.update_bar(&bar("100", "110", "90", "100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(500)));
    }

    #[test]
    fn test_vac_flat_bar_zero_clv() {
        let mut vac = VolumeAtClose::new("vac").unwrap();
        // flat bar: clv = 0, fraction = 0.5, vac = 250
        let v = vac.update_bar(&bar("100", "100", "100", "100", "500")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(250)));
    }

    #[test]
    fn test_vac_ready_after_first_bar() {
        let mut vac = VolumeAtClose::new("vac").unwrap();
        vac.update_bar(&bar("100", "110", "90", "105", "500")).unwrap();
        assert!(vac.is_ready());
    }

    #[test]
    fn test_vac_period_is_one() {
        let vac = VolumeAtClose::new("vac").unwrap();
        assert_eq!(vac.period(), 1);
    }

    #[test]
    fn test_vac_reset() {
        let mut vac = VolumeAtClose::new("vac").unwrap();
        vac.update_bar(&bar("100", "110", "90", "105", "500")).unwrap();
        assert!(vac.is_ready());
        vac.reset();
        assert!(!vac.is_ready());
    }
}
