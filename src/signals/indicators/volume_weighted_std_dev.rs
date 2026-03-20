//! Volume-Weighted Standard Deviation indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume-Weighted Standard Deviation — measures price dispersion weighted by
/// trading volume, giving more weight to price levels with higher activity.
///
/// ```text
/// VWAP  = Σ(close × volume) / Σ(volume)
/// VWVar = Σ(volume × (close − VWAP)²) / Σ(volume)
/// VWSD  = √VWVar
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// if total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeWeightedStdDev;
/// use fin_primitives::signals::Signal;
///
/// let v = VolumeWeightedStdDev::new("vwsd", 20).unwrap();
/// assert_eq!(v.period(), 20);
/// ```
pub struct VolumeWeightedStdDev {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    volumes: VecDeque<Decimal>,
}

impl VolumeWeightedStdDev {
    /// Constructs a new `VolumeWeightedStdDev`.
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
            closes: VecDeque::with_capacity(period),
            volumes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolumeWeightedStdDev {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }
        self.volumes.push_back(bar.volume);
        if self.volumes.len() > self.period { self.volumes.pop_front(); }

        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let total_vol: Decimal = self.volumes.iter().sum();
        if total_vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let vwap: Decimal = self.closes.iter().zip(self.volumes.iter())
            .map(|(c, v)| *c * *v)
            .sum::<Decimal>() / total_vol;

        let vw_var: Decimal = self.closes.iter().zip(self.volumes.iter())
            .map(|(c, v)| { let d = *c - vwap; *v * d * d })
            .sum::<Decimal>() / total_vol;

        let vw_var_f = match vw_var.to_f64() {
            Some(f) => f,
            None => return Ok(SignalValue::Unavailable),
        };
        let std_dev = match Decimal::from_f64(vw_var_f.sqrt()) {
            Some(d) => d,
            None => return Ok(SignalValue::Unavailable),
        };
        Ok(SignalValue::Scalar(std_dev))
    }

    fn reset(&mut self) {
        self.closes.clear();
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

    fn bar(c: &str, v: &str) -> OhlcvBar {
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let vq = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: cp, low: cp, close: cp,
            volume: vq,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vwsd_invalid_period() {
        assert!(VolumeWeightedStdDev::new("v", 0).is_err());
    }

    #[test]
    fn test_vwsd_unavailable_before_warm_up() {
        let mut v = VolumeWeightedStdDev::new("v", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(v.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vwsd_constant_price_gives_zero() {
        let mut v = VolumeWeightedStdDev::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = v.update_bar(&bar("100", "1000")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vwsd_varying_prices_positive() {
        let mut v = VolumeWeightedStdDev::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        let data = [("90", "1000"), ("100", "1000"), ("110", "1000")];
        for (c, vol) in &data {
            last = v.update_bar(&bar(c, vol)).unwrap();
        }
        if let SignalValue::Scalar(s) = last {
            assert!(s > dec!(0), "VWSD should be positive with varying prices: {}", s);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vwsd_reset() {
        let mut v = VolumeWeightedStdDev::new("v", 3).unwrap();
        for _ in 0..3 { v.update_bar(&bar("100", "1000")).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
