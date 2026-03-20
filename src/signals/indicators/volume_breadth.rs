//! Volume Breadth indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Breadth — fraction of total volume that is "up volume" over a window.
///
/// ```text
/// up_vol_t    = volume_t  if close_t >= open_t  else  0
/// output      = sum(up_vol, period) / sum(volume, period)
/// ```
///
/// Values near 1.0 indicate predominantly bullish volume; near 0 bearish.
/// Returns 0.5 when total volume is zero.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeBreadth;
/// use fin_primitives::signals::Signal;
///
/// let vb = VolumeBreadth::new("vb", 10).unwrap();
/// assert_eq!(vb.period(), 10);
/// ```
pub struct VolumeBreadth {
    name: String,
    period: usize,
    up_vols: VecDeque<Decimal>,
    total_vols: VecDeque<Decimal>,
}

impl VolumeBreadth {
    /// Creates a new `VolumeBreadth`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            up_vols: VecDeque::with_capacity(period),
            total_vols: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolumeBreadth {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let up_vol = if bar.close >= bar.open { bar.volume } else { Decimal::ZERO };
        self.up_vols.push_back(up_vol);
        self.total_vols.push_back(bar.volume);
        if self.up_vols.len() > self.period { self.up_vols.pop_front(); }
        if self.total_vols.len() > self.period { self.total_vols.pop_front(); }
        if self.up_vols.len() < self.period { return Ok(SignalValue::Unavailable); }

        let total: Decimal = self.total_vols.iter().sum();
        if total.is_zero() {
            return Ok(SignalValue::Scalar(
                Decimal::ONE / Decimal::from(2u32)
            ));
        }

        let up: Decimal = self.up_vols.iter().sum();
        Ok(SignalValue::Scalar(up / total))
    }

    fn is_ready(&self) -> bool { self.up_vols.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.up_vols.clear();
        self.total_vols.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_ocv(o: &str, c: &str, v: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let vol = Quantity::new(v.parse().unwrap()).unwrap();
        let ov: rust_decimal::Decimal = o.parse().unwrap();
        let cv: rust_decimal::Decimal = c.parse().unwrap();
        let hp = Price::new(ov.max(cv)).unwrap();
        let lp = Price::new(ov.min(cv)).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: vol,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vb_invalid() {
        assert!(VolumeBreadth::new("v", 0).is_err());
    }

    #[test]
    fn test_vb_unavailable_before_warmup() {
        let mut v = VolumeBreadth::new("v", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(v.update_bar(&bar_ocv("100", "101", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vb_all_up_is_one() {
        let mut v = VolumeBreadth::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = v.update_bar(&bar_ocv("100", "101", "1000")).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vb_all_down_is_zero() {
        let mut v = VolumeBreadth::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = v.update_bar(&bar_ocv("101", "100", "1000")).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vb_mixed_half() {
        // 2 up, 2 down → breadth = 0.5
        let mut v = VolumeBreadth::new("v", 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            v.update_bar(&bar_ocv("100", "101", "1000")).unwrap();
            last = v.update_bar(&bar_ocv("101", "100", "1000")).unwrap();
        }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(0.5));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vb_reset() {
        let mut v = VolumeBreadth::new("v", 3).unwrap();
        for _ in 0..5 { v.update_bar(&bar_ocv("100", "101", "1000")).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
