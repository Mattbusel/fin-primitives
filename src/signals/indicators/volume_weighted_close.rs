//! Volume-Weighted Close indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume-Weighted Close (VWC) — a rolling average close price weighted by volume.
///
/// ```text
/// vwc = Σ(close[i] × volume[i]) / Σ(volume[i])  over last `period` bars
/// ```
///
/// Unlike a simple moving average, VWC gives more weight to bars with higher volume,
/// making it more responsive during high-activity periods.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeWeightedClose;
/// use fin_primitives::signals::Signal;
///
/// let vwc = VolumeWeightedClose::new("vwc", 10).unwrap();
/// assert_eq!(vwc.period(), 10);
/// ```
pub struct VolumeWeightedClose {
    name: String,
    period: usize,
    // (close × volume, volume)
    history: VecDeque<(Decimal, Decimal)>,
}

impl VolumeWeightedClose {
    /// Creates a new `VolumeWeightedClose`.
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
            history: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolumeWeightedClose {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let cv = bar.close * bar.volume;
        self.history.push_back((cv, bar.volume));
        if self.history.len() > self.period {
            self.history.pop_front();
        }
        if self.history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let total_vol: Decimal = self.history.iter().map(|(_, v)| v).sum();
        if total_vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let total_cv: Decimal = self.history.iter().map(|(cv, _)| cv).sum();
        let vwc = total_cv
            .checked_div(total_vol)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(vwc))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period
    }

    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str, vol: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
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
    fn test_vwc_invalid_period() {
        assert!(VolumeWeightedClose::new("v", 0).is_err());
    }

    #[test]
    fn test_vwc_unavailable_early() {
        let mut v = VolumeWeightedClose::new("v", 3).unwrap();
        assert_eq!(v.update_bar(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(v.update_bar(&bar("110", "200")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vwc_equal_volumes_is_simple_avg() {
        // Equal volume → VWC = SMA
        let mut v = VolumeWeightedClose::new("v", 3).unwrap();
        v.update_bar(&bar("100", "100")).unwrap();
        v.update_bar(&bar("110", "100")).unwrap();
        if let SignalValue::Scalar(val) = v.update_bar(&bar("120", "100")).unwrap() {
            assert_eq!(val, dec!(110));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vwc_high_volume_bar_pulls_average() {
        // Third bar has 10x volume and price 200 — should pull VWC toward 200
        let mut v = VolumeWeightedClose::new("v", 3).unwrap();
        v.update_bar(&bar("100", "100")).unwrap();
        v.update_bar(&bar("100", "100")).unwrap();
        if let SignalValue::Scalar(val) = v.update_bar(&bar("200", "1000")).unwrap() {
            // vwc = (100*100 + 100*100 + 200*1000) / (100+100+1000) = 220000/1200 ≈ 183.33
            assert!(val > dec!(150), "high-vol bar should dominate: {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vwc_zero_volume_unavailable() {
        let mut v = VolumeWeightedClose::new("v", 2).unwrap();
        v.update_bar(&bar("100", "0")).unwrap();
        assert_eq!(v.update_bar(&bar("110", "0")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vwc_reset() {
        let mut v = VolumeWeightedClose::new("v", 2).unwrap();
        v.update_bar(&bar("100", "100")).unwrap();
        v.update_bar(&bar("110", "100")).unwrap();
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
