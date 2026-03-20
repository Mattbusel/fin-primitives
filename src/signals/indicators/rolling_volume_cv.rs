//! Rolling Volume Coefficient of Variation — std dev / mean of volume over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Volume Coefficient of Variation (CV) — `std_dev(volume) / mean(volume)`.
///
/// Measures the relative dispersion of volume over the last `period` bars:
/// - **Low CV (near 0)**: volume is consistent bar-to-bar.
/// - **High CV**: volume is erratic / spiky.
///
/// Uses population standard deviation. Returns [`SignalValue::Unavailable`] until
/// `period` bars have been seen, or when mean volume is zero, or when f64 conversion fails.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingVolumeCV;
/// use fin_primitives::signals::Signal;
/// let cv = RollingVolumeCV::new("vol_cv_14", 14).unwrap();
/// assert_eq!(cv.period(), 14);
/// ```
pub struct RollingVolumeCV {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl RollingVolumeCV {
    /// Constructs a new `RollingVolumeCV`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RollingVolumeCV {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.sum += bar.volume;
        self.window.push_back(bar.volume);
        if self.window.len() > self.period {
            let removed = self.window.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let mean_d = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if mean_d.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let mean_f = mean_d.to_f64().unwrap_or(0.0);
        let variance: f64 = self
            .window
            .iter()
            .filter_map(|v| v.to_f64())
            .map(|v| {
                let d = v - mean_f;
                d * d
            })
            .sum::<f64>()
            / n;

        let std_dev = variance.sqrt();
        let cv = std_dev / mean_f;

        Decimal::try_from(cv)
            .map(|d| SignalValue::Scalar(d.max(Decimal::ZERO)))
            .or(Ok(SignalValue::Unavailable))
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

    fn bar(vol: &str) -> OhlcvBar {
        let p = Price::new("100".parse().unwrap()).unwrap();
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
    fn test_rcv_invalid_period() {
        assert!(RollingVolumeCV::new("cv", 0).is_err());
        assert!(RollingVolumeCV::new("cv", 1).is_err());
    }

    #[test]
    fn test_rcv_unavailable_before_period() {
        let mut s = RollingVolumeCV::new("cv", 3).unwrap();
        assert_eq!(s.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("2000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rcv_constant_volume_gives_zero() {
        let mut s = RollingVolumeCV::new("cv", 3).unwrap();
        s.update_bar(&bar("1000")).unwrap();
        s.update_bar(&bar("1000")).unwrap();
        let v = s.update_bar(&bar("1000")).unwrap();
        if let SignalValue::Scalar(cv) = v {
            assert!(cv.abs() < dec!(0.0001), "constant volume should give CV ~0: {cv}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rcv_non_negative() {
        let mut s = RollingVolumeCV::new("cv", 4).unwrap();
        for vol in &["1000", "3000", "500", "2000", "4000"] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(vol)).unwrap() {
                assert!(v >= dec!(0), "CV must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_rcv_zero_volume_unavailable() {
        let mut s = RollingVolumeCV::new("cv", 2).unwrap();
        s.update_bar(&bar("0")).unwrap();
        let v = s.update_bar(&bar("0")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rcv_reset() {
        let mut s = RollingVolumeCV::new("cv", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("1000")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
