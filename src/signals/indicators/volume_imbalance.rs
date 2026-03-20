//! Volume Imbalance indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Imbalance — measures the dominance of buying vs selling volume
/// over a rolling window, normalized to `[−1, +1]`.
///
/// ```text
/// up_vol   = sum of volume on bars where close > open (over period)
/// down_vol = sum of volume on bars where close < open (over period)
/// imbalance = (up_vol − down_vol) / (up_vol + down_vol)
/// ```
///
/// Returns `+1` when all volume is on up-bars; `−1` when all on down-bars; `0` when balanced.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeImbalance;
/// use fin_primitives::signals::Signal;
///
/// let vi = VolumeImbalance::new("vi", 10).unwrap();
/// assert_eq!(vi.period(), 10);
/// ```
pub struct VolumeImbalance {
    name: String,
    period: usize,
    // Each entry: (up_vol, down_vol) for that bar
    history: VecDeque<(Decimal, Decimal)>,
}

impl VolumeImbalance {
    /// Creates a new `VolumeImbalance`.
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

impl Signal for VolumeImbalance {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let up_vol = if bar.close > bar.open { bar.volume } else { Decimal::ZERO };
        let down_vol = if bar.close < bar.open { bar.volume } else { Decimal::ZERO };

        self.history.push_back((up_vol, down_vol));
        if self.history.len() > self.period {
            self.history.pop_front();
        }
        if self.history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let total_up: Decimal = self.history.iter().map(|(u, _)| u).sum();
        let total_down: Decimal = self.history.iter().map(|(_, d)| d).sum();
        let total = total_up + total_down;

        if total.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let imbalance = (total_up - total_down)
            .checked_div(total)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(imbalance))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

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

    fn bar(o: &str, c: &str, vol: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let hp = if cp >= op { cp } else { op };
        let lp = if cp <= op { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vi_invalid_period() {
        assert!(VolumeImbalance::new("v", 0).is_err());
    }

    #[test]
    fn test_vi_unavailable_early() {
        let mut vi = VolumeImbalance::new("v", 3).unwrap();
        assert_eq!(vi.update_bar(&bar("100", "105", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(vi.update_bar(&bar("100", "105", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vi_all_up_returns_one() {
        let mut vi = VolumeImbalance::new("v", 3).unwrap();
        for _ in 0..3 { vi.update_bar(&bar("100", "105", "100")).unwrap(); }
        if let SignalValue::Scalar(v) = vi.update_bar(&bar("100", "105", "100")).unwrap() {
            assert_eq!(v, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vi_all_down_returns_neg_one() {
        let mut vi = VolumeImbalance::new("v", 3).unwrap();
        for _ in 0..3 { vi.update_bar(&bar("105", "100", "100")).unwrap(); }
        if let SignalValue::Scalar(v) = vi.update_bar(&bar("105", "100", "100")).unwrap() {
            assert_eq!(v, dec!(-1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vi_balanced_near_zero() {
        let mut vi = VolumeImbalance::new("v", 2).unwrap();
        vi.update_bar(&bar("100", "105", "100")).unwrap(); // up bar
        if let SignalValue::Scalar(v) = vi.update_bar(&bar("105", "100", "100")).unwrap() {
            // Equal up/down volume → 0
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vi_reset() {
        let mut vi = VolumeImbalance::new("v", 2).unwrap();
        vi.update_bar(&bar("100", "105", "100")).unwrap();
        vi.update_bar(&bar("100", "105", "100")).unwrap();
        assert!(vi.is_ready());
        vi.reset();
        assert!(!vi.is_ready());
    }
}
