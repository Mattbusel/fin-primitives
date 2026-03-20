//! Volume-Weighted Range — average bar range weighted by volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume-Weighted Range — `sum((high - low) * volume) / sum(volume)` over N bars.
///
/// Computes the average bar range weighted by volume, so high-volume bars contribute
/// proportionally more to the average:
/// - **Higher than average range**: recent volatile bars were also high volume.
/// - **Lower than average range**: recent quiet bars absorbed more volume.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or when
/// total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeWeightedRange;
/// use fin_primitives::signals::Signal;
/// let vwr = VolumeWeightedRange::new("vwr_14", 14).unwrap();
/// assert_eq!(vwr.period(), 14);
/// ```
pub struct VolumeWeightedRange {
    name: String,
    period: usize,
    window: VecDeque<(Decimal, Decimal)>, // (range*vol, volume)
    range_vol_sum: Decimal,
    vol_sum: Decimal,
}

impl VolumeWeightedRange {
    /// Constructs a new `VolumeWeightedRange`.
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
            range_vol_sum: Decimal::ZERO,
            vol_sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeWeightedRange {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let rv = range * bar.volume;
        self.range_vol_sum += rv;
        self.vol_sum += bar.volume;
        self.window.push_back((rv, bar.volume));

        if self.window.len() > self.period {
            let (orv, ov) = self.window.pop_front().unwrap();
            self.range_vol_sum -= orv;
            self.vol_sum -= ov;
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.vol_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let vwr = self.range_vol_sum
            .checked_div(self.vol_sum)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(vwr.max(Decimal::ZERO)))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.range_vol_sum = Decimal::ZERO;
        self.vol_sum = Decimal::ZERO;
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
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vwr_invalid_period() {
        assert!(VolumeWeightedRange::new("vwr", 0).is_err());
    }

    #[test]
    fn test_vwr_unavailable_before_period() {
        let mut s = VolumeWeightedRange::new("vwr", 3).unwrap();
        assert_eq!(s.update_bar(&bar("110","90","1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("110","90","1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vwr_uniform_volume_equals_avg_range() {
        let mut s = VolumeWeightedRange::new("vwr", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("110","90","1000")).unwrap(); }
        let v = s.update_bar(&bar("110","90","1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_vwr_high_vol_wide_bar_dominates() {
        let mut s = VolumeWeightedRange::new("vwr", 2).unwrap();
        s.update_bar(&bar("105","95","100")).unwrap();
        let v = s.update_bar(&bar("120","80","10000")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(20), "high-vol wide bar should dominate VWR: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vwr_zero_volume_unavailable() {
        let mut s = VolumeWeightedRange::new("vwr", 2).unwrap();
        s.update_bar(&bar("110","90","0")).unwrap();
        let v = s.update_bar(&bar("110","90","0")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_vwr_reset() {
        let mut s = VolumeWeightedRange::new("vwr", 2).unwrap();
        for _ in 0..2 { s.update_bar(&bar("110","90","1000")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
