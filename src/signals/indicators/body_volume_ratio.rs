//! Body-to-Volume Ratio indicator.
//!
//! Measures price movement per unit of volume, quantifying how efficiently
//! volume is translated into directional price movement.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Body-to-Volume Ratio: `|close - open| / volume`.
///
/// Quantifies how much absolute price movement (body size) was produced per
/// unit of volume traded. A high ratio indicates that each unit of volume
/// drove significant price movement (efficient price discovery). A low ratio
/// indicates heavy volume with little net price change (absorption or
/// indecision).
///
/// Returns [`SignalValue::Unavailable`] when `volume` is zero (no trading activity).
/// Always ready after the first bar (period = 1).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0` (not applicable here,
/// kept for API consistency).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyVolumeRatio;
/// use fin_primitives::signals::Signal;
///
/// let bvr = BodyVolumeRatio::new("bvr").unwrap();
/// assert_eq!(bvr.period(), 1);
/// ```
pub struct BodyVolumeRatio {
    name: String,
    ready: bool,
}

impl BodyVolumeRatio {
    /// Constructs a new `BodyVolumeRatio`.
    ///
    /// # Errors
    /// Never fails; returns `Ok` always. Signature kept consistent with other indicators.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), ready: false })
    }
}

impl Signal for BodyVolumeRatio {
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
        self.ready = true;

        let body = (bar.close - bar.open).abs();
        let vol = bar.volume;

        if vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = body
            .checked_div(vol)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio))
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

    fn bar(open: &str, close: &str, vol: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let high = if c >= o { c } else { o };
        let low = if c <= o { c } else { o };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o,
            high,
            low,
            close: c,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bvr_zero_volume_returns_unavailable() {
        let mut bvr = BodyVolumeRatio::new("bvr").unwrap();
        let v = bvr.update_bar(&bar("100", "105", "0")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_bvr_ready_after_first_bar() {
        let mut bvr = BodyVolumeRatio::new("bvr").unwrap();
        bvr.update_bar(&bar("100", "105", "1000")).unwrap();
        assert!(bvr.is_ready());
    }

    #[test]
    fn test_bvr_correct_ratio() {
        let mut bvr = BodyVolumeRatio::new("bvr").unwrap();
        // body = |105 - 100| = 5, volume = 1000, ratio = 0.005
        let v = bvr.update_bar(&bar("100", "105", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.005)));
    }

    #[test]
    fn test_bvr_bearish_bar_same_as_bullish() {
        let mut bvr = BodyVolumeRatio::new("bvr").unwrap();
        // body = |95 - 100| = 5, same ratio
        let v = bvr.update_bar(&bar("100", "95", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.005)));
    }

    #[test]
    fn test_bvr_doji_zero_body() {
        let mut bvr = BodyVolumeRatio::new("bvr").unwrap();
        // doji: open == close, body = 0, ratio = 0
        let v = bvr.update_bar(&bar("100", "100", "500")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bvr_period_is_one() {
        let bvr = BodyVolumeRatio::new("bvr").unwrap();
        assert_eq!(bvr.period(), 1);
    }

    #[test]
    fn test_bvr_reset() {
        let mut bvr = BodyVolumeRatio::new("bvr").unwrap();
        bvr.update_bar(&bar("100", "105", "1000")).unwrap();
        assert!(bvr.is_ready());
        bvr.reset();
        assert!(!bvr.is_ready());
    }
}
