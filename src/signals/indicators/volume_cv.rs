//! Volume Coefficient of Variation indicator.
//!
//! Rolling coefficient of variation (CV) of volume: `std(vol) / mean(vol)`.
//! Measures how erratic or stable volume is relative to its own average.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use std::collections::VecDeque;
use rust_decimal::Decimal;

/// Volume CV — rolling `std(volume) / mean(volume)`.
///
/// ```text
/// CV[t] = std(vol[t-period+1..t]) / mean(vol[t-period+1..t])
/// ```
///
/// - **High CV (> 1.0)**: erratic, spiky volume — events, news, or large
///   institutional orders dominate.
/// - **Low CV (near 0)**: very consistent volume — steady, predictable
///   participation (e.g. algo-driven liquidity).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars are collected,
/// or when mean volume is zero (no activity).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeCv;
/// use fin_primitives::signals::Signal;
/// let vcv = VolumeCv::new("vcv_20", 20).unwrap();
/// assert_eq!(vcv.period(), 20);
/// ```
pub struct VolumeCv {
    name: String,
    period: usize,
    vols: VecDeque<f64>,
}

impl VolumeCv {
    /// Constructs a new `VolumeCv`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, vols: VecDeque::with_capacity(period) })
    }
}

impl Signal for VolumeCv {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.vols.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let v = bar.volume.to_f64().unwrap_or(0.0);
        self.vols.push_back(v);
        if self.vols.len() > self.period {
            self.vols.pop_front();
        }

        if self.vols.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.vols.len() as f64;
        let mean = self.vols.iter().sum::<f64>() / n;
        if mean == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let variance = self.vols.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        let cv = variance.sqrt() / mean;

        Decimal::try_from(cv)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn reset(&mut self) {
        self.vols.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(vol: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vcv_invalid_period() {
        assert!(VolumeCv::new("vcv", 0).is_err());
        assert!(VolumeCv::new("vcv", 1).is_err());
    }

    #[test]
    fn test_vcv_unavailable_during_warmup() {
        let mut vcv = VolumeCv::new("vcv", 3).unwrap();
        assert_eq!(vcv.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(vcv.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
        assert!(!vcv.is_ready());
    }

    #[test]
    fn test_vcv_constant_volume_zero() {
        // Same volume → std = 0 → CV = 0
        let mut vcv = VolumeCv::new("vcv", 3).unwrap();
        for _ in 0..3 { vcv.update_bar(&bar("1000")).unwrap(); }
        if let SignalValue::Scalar(v) = vcv.update_bar(&bar("1000")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vcv_zero_volume_unavailable() {
        let mut vcv = VolumeCv::new("vcv", 2).unwrap();
        vcv.update_bar(&bar("0")).unwrap();
        assert_eq!(vcv.update_bar(&bar("0")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vcv_varied_volume_positive() {
        // Alternating 100 and 1000 → high CV
        let mut vcv = VolumeCv::new("vcv", 4).unwrap();
        for v in &["100", "1000", "100", "1000"] { vcv.update_bar(&bar(v)).unwrap(); }
        if let SignalValue::Scalar(v) = vcv.update_bar(&bar("100")).unwrap() {
            assert!(v > dec!(0), "varied volume → positive CV: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vcv_reset() {
        let mut vcv = VolumeCv::new("vcv", 2).unwrap();
        vcv.update_bar(&bar("1000")).unwrap();
        vcv.update_bar(&bar("1100")).unwrap();
        assert!(vcv.is_ready());
        vcv.reset();
        assert!(!vcv.is_ready());
    }
}
