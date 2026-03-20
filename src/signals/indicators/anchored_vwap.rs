//! Anchored VWAP indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Anchored VWAP — VWAP computed from a fixed anchor bar onward.
///
/// Unlike rolling VWAP, this resets only when `reset()` is called explicitly,
/// making it suitable for anchoring to a specific reference point (e.g. a swing
/// low, earnings date, or session open).
///
/// ```text
/// AVWAP = Σ(typical_price × volume) / Σ(volume)
/// typical_price = (high + low + close) / 3
/// ```
///
/// Returns [`SignalValue::Unavailable`] until at least one bar with non-zero volume
/// has been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AnchoredVwap;
/// use fin_primitives::signals::Signal;
///
/// let av = AnchoredVwap::new("avwap").unwrap();
/// assert_eq!(av.period(), 1);
/// ```
pub struct AnchoredVwap {
    name: String,
    cum_tp_vol: Decimal,
    cum_vol: Decimal,
}

impl AnchoredVwap {
    /// Creates a new `AnchoredVwap`.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            cum_tp_vol: Decimal::ZERO,
            cum_vol: Decimal::ZERO,
        })
    }
}

impl Signal for AnchoredVwap {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.volume > Decimal::ZERO {
            let tp = bar.typical_price();
            self.cum_tp_vol += tp * bar.volume;
            self.cum_vol += bar.volume;
        }
        if self.cum_vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.cum_tp_vol / self.cum_vol))
    }

    fn is_ready(&self) -> bool { !self.cum_vol.is_zero() }
    fn period(&self) -> usize { 1 }

    fn reset(&mut self) {
        self.cum_tp_vol = Decimal::ZERO;
        self.cum_vol = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hlcv(h: &str, l: &str, c: &str, v: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
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

    fn bar_no_vol(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_avwap_unavailable_zero_volume() {
        let mut av = AnchoredVwap::new("av").unwrap();
        assert_eq!(av.update_bar(&bar_no_vol("100")).unwrap(), SignalValue::Unavailable);
        assert!(!av.is_ready());
    }

    #[test]
    fn test_avwap_single_bar() {
        // tp = (110+90+100)/3 = 100, vol = 1000 → avwap = 100
        let mut av = AnchoredVwap::new("av").unwrap();
        if let SignalValue::Scalar(v) = av.update_bar(&bar_hlcv("110", "90", "100", "1000")).unwrap() {
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_avwap_accumulates() {
        let mut av = AnchoredVwap::new("av").unwrap();
        // bar1: tp=100, vol=1000 → cum_tp_vol=100000, cum_vol=1000
        av.update_bar(&bar_hlcv("110", "90", "100", "1000")).unwrap();
        // bar2: tp=110, vol=2000 → cum_tp_vol=100000+220000=320000, cum_vol=3000 → avwap=106.67
        if let SignalValue::Scalar(v) = av.update_bar(&bar_hlcv("120", "100", "110", "2000")).unwrap() {
            assert!(v > dec!(100) && v < dec!(110), "unexpected avwap {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_avwap_reset_reanchors() {
        let mut av = AnchoredVwap::new("av").unwrap();
        av.update_bar(&bar_hlcv("110", "90", "100", "1000")).unwrap();
        assert!(av.is_ready());
        av.reset();
        assert!(!av.is_ready());
        // After reset, first bar re-anchors
        if let SignalValue::Scalar(v) = av.update_bar(&bar_hlcv("200", "180", "190", "500")).unwrap() {
            assert_eq!(v, dec!(190));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_avwap_skips_zero_volume_bars() {
        let mut av = AnchoredVwap::new("av").unwrap();
        av.update_bar(&bar_hlcv("110", "90", "100", "1000")).unwrap();
        // Zero-volume bar should not change AVWAP
        let v1 = av.update_bar(&bar_no_vol("200")).unwrap();
        if let SignalValue::Scalar(v) = v1 {
            assert_eq!(v, dec!(100)); // unchanged from first bar
        } else { panic!("expected Scalar"); }
    }
}
