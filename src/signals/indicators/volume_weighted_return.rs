//! Volume-Weighted Return indicator.
//!
//! Computes the rolling volume-weighted mean of `(close - open)` per bar,
//! measuring the average net move per share weighted by trading activity.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling volume-weighted mean return: `sum(vol × (close − open), N) / sum(vol, N)`.
///
/// Weights each bar's net move `(close - open)` by its volume. Bars with heavy
/// volume have proportionally more influence on the result. Positive values
/// indicate net buying pressure dominated the window; negative values indicate
/// net selling pressure.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have accumulated or
/// when cumulative volume is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeWeightedReturn;
/// use fin_primitives::signals::Signal;
///
/// let vwr = VolumeWeightedReturn::new("vwr", 14).unwrap();
/// assert_eq!(vwr.period(), 14);
/// assert!(!vwr.is_ready());
/// ```
pub struct VolumeWeightedReturn {
    name: String,
    period: usize,
    vol_ret_window: VecDeque<Decimal>,
    vol_window: VecDeque<Decimal>,
    vol_ret_sum: Decimal,
    vol_sum: Decimal,
}

impl VolumeWeightedReturn {
    /// Constructs a new `VolumeWeightedReturn`.
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
            vol_ret_window: VecDeque::with_capacity(period),
            vol_window: VecDeque::with_capacity(period),
            vol_ret_sum: Decimal::ZERO,
            vol_sum: Decimal::ZERO,
        })
    }
}

impl crate::signals::Signal for VolumeWeightedReturn {
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
        let net_move = bar.close - bar.open;
        let vol = bar.volume;
        let vol_ret = net_move * vol;

        self.vol_ret_sum += vol_ret;
        self.vol_sum += vol;

        self.vol_ret_window.push_back(vol_ret);
        self.vol_window.push_back(vol);

        if self.vol_ret_window.len() > self.period {
            if let Some(old_vr) = self.vol_ret_window.pop_front() {
                self.vol_ret_sum -= old_vr;
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

        let vwr = self.vol_ret_sum
            .checked_div(self.vol_sum)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(vwr))
    }

    fn reset(&mut self) {
        self.vol_ret_window.clear();
        self.vol_window.clear();
        self.vol_ret_sum = Decimal::ZERO;
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

    fn bar(open: &str, close: &str, vol: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let (high, low) = if c >= o { (c, o) } else { (o, c) };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o, high, low, close: c,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vwr_invalid_period() {
        assert!(VolumeWeightedReturn::new("vwr", 0).is_err());
    }

    #[test]
    fn test_vwr_unavailable_during_warmup() {
        let mut vwr = VolumeWeightedReturn::new("vwr", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(vwr.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vwr_equal_volume_is_mean_return() {
        // All bars equal volume → VWR == mean of (close-open)
        let mut vwr = VolumeWeightedReturn::new("vwr", 3).unwrap();
        vwr.update_bar(&bar("100", "106", "1000")).unwrap(); // +6
        vwr.update_bar(&bar("100", "104", "1000")).unwrap(); // +4
        let v = vwr.update_bar(&bar("100", "104", "1000")).unwrap(); // +4 → mean = 14/3
        if let SignalValue::Scalar(s) = v {
            // (6+4+4)/3 ≈ 4.666...
            let expected = dec!(14) / dec!(3);
            let diff = (s - expected).abs();
            assert!(diff < dec!(0.0001), "expected ~4.667, got {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vwr_high_vol_dominates() {
        let mut vwr = VolumeWeightedReturn::new("vwr", 2).unwrap();
        vwr.update_bar(&bar("100", "90", "10")).unwrap(); // -10, vol=10
        let v = vwr.update_bar(&bar("100", "110", "1000")).unwrap(); // +10, vol=1000
        // VWR should be close to +10
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(9), "high-vol bullish bar should dominate: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vwr_zero_volume_unavailable() {
        let mut vwr = VolumeWeightedReturn::new("vwr", 2).unwrap();
        vwr.update_bar(&bar("100", "105", "0")).unwrap();
        let v = vwr.update_bar(&bar("100", "105", "0")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_vwr_reset() {
        let mut vwr = VolumeWeightedReturn::new("vwr", 3).unwrap();
        for _ in 0..3 {
            vwr.update_bar(&bar("100", "105", "1000")).unwrap();
        }
        assert!(vwr.is_ready());
        vwr.reset();
        assert!(!vwr.is_ready());
    }
}
