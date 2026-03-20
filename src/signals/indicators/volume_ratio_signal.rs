//! Volume Ratio Signal indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Ratio Signal — fraction of total volume over `period` bars that
/// occurred on up-bars (bars where `close >= open`).
///
/// A value near 1.0 indicates overwhelming buying volume; near 0 indicates
/// selling dominance. Unlike raw OBV, this normalizes for the period.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// if total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeRatioSignal;
/// use fin_primitives::signals::Signal;
///
/// let vrs = VolumeRatioSignal::new("vrs", 10).unwrap();
/// assert_eq!(vrs.period(), 10);
/// ```
pub struct VolumeRatioSignal {
    name: String,
    period: usize,
    up_vols: VecDeque<Decimal>,
    all_vols: VecDeque<Decimal>,
}

impl VolumeRatioSignal {
    /// Constructs a new `VolumeRatioSignal`.
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
            up_vols: VecDeque::with_capacity(period),
            all_vols: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolumeRatioSignal {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.all_vols.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let up_vol = if bar.close >= bar.open { bar.volume } else { Decimal::ZERO };
        self.up_vols.push_back(up_vol);
        self.all_vols.push_back(bar.volume);
        if self.up_vols.len() > self.period { self.up_vols.pop_front(); }
        if self.all_vols.len() > self.period { self.all_vols.pop_front(); }

        if self.all_vols.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let total: Decimal = self.all_vols.iter().sum();
        if total.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let up_total: Decimal = self.up_vols.iter().sum();
        Ok(SignalValue::Scalar(up_total / total))
    }

    fn reset(&mut self) {
        self.up_vols.clear();
        self.all_vols.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str, v: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let vq = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp, low: op, close: cp,
            volume: vq,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vrs_invalid_period() {
        assert!(VolumeRatioSignal::new("vrs", 0).is_err());
    }

    #[test]
    fn test_vrs_unavailable_before_warm_up() {
        let mut vrs = VolumeRatioSignal::new("vrs", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(vrs.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vrs_all_up_bars() {
        let mut vrs = VolumeRatioSignal::new("vrs", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = vrs.update_bar(&bar("100", "105", "1000")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_vrs_all_down_bars() {
        let mut vrs = VolumeRatioSignal::new("vrs", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = vrs.update_bar(&bar("105", "100", "1000")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vrs_reset() {
        let mut vrs = VolumeRatioSignal::new("vrs", 3).unwrap();
        for _ in 0..3 { vrs.update_bar(&bar("100", "105", "1000")).unwrap(); }
        assert!(vrs.is_ready());
        vrs.reset();
        assert!(!vrs.is_ready());
    }
}
