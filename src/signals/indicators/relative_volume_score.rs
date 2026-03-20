//! Relative Volume Score indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Relative Volume Score — the ratio of the current bar's volume to the simple
/// moving average of volume over the last `period` bars.
///
/// ```text
/// RVS = volume / SMA(volume, n)
/// ```
///
/// Values > 1 indicate above-average volume; < 1 indicate below-average.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or average is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RelativeVolumeScore;
/// use fin_primitives::signals::Signal;
///
/// let rvs = RelativeVolumeScore::new("rvs", 20).unwrap();
/// assert_eq!(rvs.period(), 20);
/// ```
pub struct RelativeVolumeScore {
    name: String,
    period: usize,
    volumes: VecDeque<Decimal>,
    sum: Decimal,
}

impl RelativeVolumeScore {
    /// Constructs a new `RelativeVolumeScore`.
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
            volumes: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RelativeVolumeScore {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.volumes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.volumes.push_back(bar.volume);
        self.sum += bar.volume;
        if self.volumes.len() > self.period {
            self.sum -= self.volumes.pop_front().unwrap();
        }
        if self.volumes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let nd = Decimal::from(self.period as u32);
        let avg = self.sum / nd;
        if avg.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(bar.volume / avg))
    }

    fn reset(&mut self) {
        self.volumes.clear();
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

    fn bar(v: &str) -> OhlcvBar {
        let p = Price::new("100".parse().unwrap()).unwrap();
        let vq = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: vq,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rvs_invalid_period() {
        assert!(RelativeVolumeScore::new("rvs", 0).is_err());
    }

    #[test]
    fn test_rvs_unavailable_before_warm_up() {
        let mut rvs = RelativeVolumeScore::new("rvs", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(rvs.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rvs_constant_volume_gives_one() {
        let mut rvs = RelativeVolumeScore::new("rvs", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = rvs.update_bar(&bar("1000")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_rvs_double_volume() {
        // 2 bars at 1000, then 1 bar at 2000 → avg=1333..., last/avg = 2000/1333 ≈ 1.5
        let mut rvs = RelativeVolumeScore::new("rvs", 3).unwrap();
        rvs.update_bar(&bar("1000")).unwrap();
        rvs.update_bar(&bar("1000")).unwrap();
        let result = rvs.update_bar(&bar("2000")).unwrap();
        if let SignalValue::Scalar(v) = result {
            assert!(v > dec!(1), "above-average volume should give score > 1: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rvs_reset() {
        let mut rvs = RelativeVolumeScore::new("rvs", 3).unwrap();
        for _ in 0..3 { rvs.update_bar(&bar("1000")).unwrap(); }
        assert!(rvs.is_ready());
        rvs.reset();
        assert!(!rvs.is_ready());
    }
}
