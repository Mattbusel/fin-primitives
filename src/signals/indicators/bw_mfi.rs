//! Williams' Market Facilitation Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Williams' Market Facilitation Index (BW MFI).
///
/// ```text
/// BW_MFI_t = (high_t − low_t) / volume_t
/// ```
///
/// Measures the efficiency of price movement per unit of volume.
/// High values indicate price moves easily per unit of volume (strong trend).
/// Low values indicate choppy price action with high volume (potential reversal).
///
/// Returns [`SignalValue::Scalar`] on every bar (no warmup needed, except zero volume).
/// Returns [`SignalValue::Unavailable`] when volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BwMfi;
/// use fin_primitives::signals::Signal;
///
/// let m = BwMfi::new("bwmfi").unwrap();
/// assert_eq!(m.period(), 1);
/// ```
pub struct BwMfi {
    name: String,
    ready: bool,
}

impl BwMfi {
    /// Creates a new `BwMfi`.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), ready: false })
    }
}

impl Signal for BwMfi {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.volume.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        self.ready = true;
        let range = bar.range();
        Ok(SignalValue::Scalar(range / bar.volume))
    }

    fn is_ready(&self) -> bool { self.ready }
    fn period(&self) -> usize { 1 }

    fn reset(&mut self) {
        self.ready = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hlv(h: &str, l: &str, v: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = lp;
        let vol = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: vol,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn bar_zero_vol(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: lp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bwmfi_basic() {
        let mut m = BwMfi::new("m").unwrap();
        // range=10, vol=1000 → mfi = 0.01
        if let SignalValue::Scalar(v) = m.update_bar(&bar_hlv("110", "100", "1000")).unwrap() {
            assert_eq!(v, dec!(0.01));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_bwmfi_zero_volume_unavailable() {
        let mut m = BwMfi::new("m").unwrap();
        assert_eq!(m.update_bar(&bar_zero_vol("110", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bwmfi_flat_bar_is_zero() {
        let mut m = BwMfi::new("m").unwrap();
        // h=l=100, vol=1000 → range=0 → mfi=0
        if let SignalValue::Scalar(v) = m.update_bar(&bar_hlv("100", "100", "1000")).unwrap() {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_bwmfi_reset() {
        let mut m = BwMfi::new("m").unwrap();
        m.update_bar(&bar_hlv("110", "100", "1000")).unwrap();
        assert!(m.is_ready());
        m.reset();
        assert!(!m.is_ready());
    }
}
