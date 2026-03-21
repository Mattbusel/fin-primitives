//! Volume-Weighted High-Low indicator.
//!
//! Computes a volume-weighted average of the bar midpoints (high+low)/2 over
//! a rolling window — a price level estimate that emphasizes where trading
//! activity was heaviest.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume-Weighted High-Low (VWHL): `sum(midpoint × volume, N) / sum(volume, N)`.
///
/// The midpoint of each bar is `(high + low) / 2`. Weighting by volume
/// gives more influence to bars with heavier trading activity. This can
/// be thought of as a volume-weighted median price approximation.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen
/// or when cumulative volume is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeWeightedHl;
/// use fin_primitives::signals::Signal;
///
/// let vwhl = VolumeWeightedHl::new("vwhl", 14).unwrap();
/// assert_eq!(vwhl.period(), 14);
/// assert!(!vwhl.is_ready());
/// ```
pub struct VolumeWeightedHl {
    name: String,
    period: usize,
    mid_vol_window: VecDeque<Decimal>,
    vol_window: VecDeque<Decimal>,
    mid_vol_sum: Decimal,
    vol_sum: Decimal,
}

impl VolumeWeightedHl {
    /// Constructs a new `VolumeWeightedHl`.
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
            mid_vol_window: VecDeque::with_capacity(period),
            vol_window: VecDeque::with_capacity(period),
            mid_vol_sum: Decimal::ZERO,
            vol_sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeWeightedHl {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.vol_window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = bar.midpoint();
        let vol = bar.volume;
        let mid_vol = mid * vol;

        self.mid_vol_sum += mid_vol;
        self.vol_sum += vol;

        self.mid_vol_window.push_back(mid_vol);
        self.vol_window.push_back(vol);

        if self.mid_vol_window.len() > self.period {
            if let Some(old_mv) = self.mid_vol_window.pop_front() {
                self.mid_vol_sum -= old_mv;
            }
            if let Some(old_v) = self.vol_window.pop_front() {
                self.vol_sum -= old_v;
            }
        }

        if self.vol_window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.vol_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let vwhl = self
            .mid_vol_sum
            .checked_div(self.vol_sum)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(vwhl))
    }

    fn reset(&mut self) {
        self.mid_vol_window.clear();
        self.vol_window.clear();
        self.mid_vol_sum = Decimal::ZERO;
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

    fn bar(high: &str, low: &str, vol: &str) -> OhlcvBar {
        let h = Price::new(high.parse().unwrap()).unwrap();
        let l = Price::new(low.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l, high: h, low: l, close: h,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vwhl_invalid_period() {
        assert!(VolumeWeightedHl::new("vwhl", 0).is_err());
    }

    #[test]
    fn test_vwhl_unavailable_during_warmup() {
        let mut vwhl = VolumeWeightedHl::new("vwhl", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(vwhl.update_bar(&bar("110", "90", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vwhl_constant_midpoint() {
        let mut vwhl = VolumeWeightedHl::new("vwhl", 3).unwrap();
        // All bars: high=110, low=90, mid=100
        for _ in 0..3 {
            vwhl.update_bar(&bar("110", "90", "1000")).unwrap();
        }
        let v = vwhl.update_bar(&bar("110", "90", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_vwhl_high_volume_bar_dominates() {
        let mut vwhl = VolumeWeightedHl::new("vwhl", 2).unwrap();
        // Bar 1: mid=100, vol=10
        // Bar 2: mid=200, vol=1000 → VWHL should be much closer to 200
        vwhl.update_bar(&bar("110", "90", "10")).unwrap();
        let v = vwhl.update_bar(&bar("210", "190", "1000")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(190), "high-vol bar should dominate VWHL: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vwhl_zero_volume_unavailable() {
        let mut vwhl = VolumeWeightedHl::new("vwhl", 2).unwrap();
        vwhl.update_bar(&bar("110", "90", "0")).unwrap();
        let v = vwhl.update_bar(&bar("110", "90", "0")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_vwhl_reset() {
        let mut vwhl = VolumeWeightedHl::new("vwhl", 3).unwrap();
        for _ in 0..3 {
            vwhl.update_bar(&bar("110", "90", "1000")).unwrap();
        }
        assert!(vwhl.is_ready());
        vwhl.reset();
        assert!(!vwhl.is_ready());
    }
}
