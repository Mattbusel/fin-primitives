//! Volume Density indicator -- rolling average of volume per unit price range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Density -- rolling average of `volume / (high - low)` over `period` bars.
///
/// Measures how much volume occurs per unit of price range. Higher values indicate
/// dense trading at a tight range (often near support/resistance). Zero-range bars
/// are excluded from the average.
///
/// ```text
/// density[t] = volume[t] / (high[t] - low[t])   (if high != low)
/// vd[t]      = SMA(density, period)              (counting only valid bars)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` valid bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeDensity;
/// use fin_primitives::signals::Signal;
/// let vd = VolumeDensity::new("vd", 10).unwrap();
/// assert_eq!(vd.period(), 10);
/// ```
pub struct VolumeDensity {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeDensity {
    /// Constructs a new `VolumeDensity`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeDensity {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        let density = bar.volume / range;
        self.window.push_back(density);
        self.sum += density;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(self.sum / Decimal::from(self.period as u32)))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, vol: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let mp = Price::new((hp.value() + lp.value()) / Decimal::TWO).unwrap();
        let v = Quantity::new(vol.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: mp, high: hp, low: lp, close: mp, volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vd_period_0_error() { assert!(VolumeDensity::new("vd", 0).is_err()); }

    #[test]
    fn test_vd_zero_range_unavailable() {
        let mut vd = VolumeDensity::new("vd", 1).unwrap();
        let hp = Price::new(dec!(100)).unwrap();
        let v = Quantity::new(dec!(1000)).unwrap();
        let b = OhlcvBar {
            symbol: crate::types::Symbol::new("X").unwrap(),
            open: hp, high: hp, low: hp, close: hp, volume: v,
            ts_open: crate::types::NanoTimestamp::new(0),
            ts_close: crate::types::NanoTimestamp::new(1),
            tick_count: 1,
        };
        assert_eq!(vd.update_bar(&b).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vd_unavailable_before_period() {
        let mut vd = VolumeDensity::new("vd", 3).unwrap();
        assert_eq!(vd.update_bar(&bar("110", "90", "2000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vd_constant_density() {
        // range=20, vol=2000 -> density=100 per bar
        let mut vd = VolumeDensity::new("vd", 3).unwrap();
        vd.update_bar(&bar("110", "90", "2000")).unwrap();
        vd.update_bar(&bar("110", "90", "2000")).unwrap();
        let v = vd.update_bar(&bar("110", "90", "2000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_vd_reset() {
        let mut vd = VolumeDensity::new("vd", 2).unwrap();
        vd.update_bar(&bar("110", "90", "2000")).unwrap();
        vd.update_bar(&bar("110", "90", "2000")).unwrap();
        assert!(vd.is_ready());
        vd.reset();
        assert!(!vd.is_ready());
    }
}
