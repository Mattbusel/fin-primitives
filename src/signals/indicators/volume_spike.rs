//! Volume Spike indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Spike — detects unusually high volume relative to a rolling average.
///
/// ```text
/// avg_vol   = mean(volume, period)
/// vol_ratio = volume_t / avg_vol
/// output    = vol_ratio
/// ```
///
/// Values > `threshold` indicate a volume spike. Use `is_spike()` for a simple
/// boolean signal. Returns 1 when volume equals the average.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeSpike;
/// use fin_primitives::signals::Signal;
///
/// let vs = VolumeSpike::new("vs", 20, "2.0".parse().unwrap()).unwrap();
/// assert_eq!(vs.period(), 20);
/// ```
pub struct VolumeSpike {
    name: String,
    period: usize,
    threshold: Decimal,
    volumes: VecDeque<Decimal>,
    last_ratio: Option<Decimal>,
}

impl VolumeSpike {
    /// Creates a new `VolumeSpike`.
    ///
    /// - `threshold`: ratio above which `is_spike()` returns `true` (e.g. `2.0`).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if `threshold` is not positive.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        threshold: Decimal,
    ) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        if threshold <= Decimal::ZERO {
            return Err(FinError::InvalidInput("threshold must be positive".into()));
        }
        Ok(Self {
            name: name.into(),
            period,
            threshold,
            volumes: VecDeque::with_capacity(period),
            last_ratio: None,
        })
    }

    /// Returns `true` if the last volume ratio exceeded the threshold.
    pub fn is_spike(&self) -> bool {
        self.last_ratio.map_or(false, |r| r >= self.threshold)
    }
}

impl Signal for VolumeSpike {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;
        self.volumes.push_back(vol);
        if self.volumes.len() > self.period { self.volumes.pop_front(); }
        if self.volumes.len() < self.period { return Ok(SignalValue::Unavailable); }

        let avg = self.volumes.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        if avg.is_zero() {
            self.last_ratio = Some(Decimal::ONE);
            return Ok(SignalValue::Scalar(Decimal::ONE));
        }
        let ratio = vol / avg;
        self.last_ratio = Some(ratio);
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool { self.volumes.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.volumes.clear();
        self.last_ratio = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_v(v: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
        let vol = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: vol,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vs_invalid() {
        assert!(VolumeSpike::new("v", 0, dec!(2)).is_err());
        assert!(VolumeSpike::new("v", 10, dec!(0)).is_err());
        assert!(VolumeSpike::new("v", 10, dec!(-1)).is_err());
    }

    #[test]
    fn test_vs_unavailable_before_warmup() {
        let mut v = VolumeSpike::new("v", 3, dec!(2)).unwrap();
        for _ in 0..2 {
            assert_eq!(v.update_bar(&bar_v("1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vs_uniform_is_one() {
        let mut v = VolumeSpike::new("v", 3, dec!(2)).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = v.update_bar(&bar_v("1000")).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(1));
        } else { panic!("expected Scalar"); }
        assert!(!v.is_spike());
    }

    #[test]
    fn test_vs_spike_detected() {
        // After 2 normal bars, spike with volume >> average
        let mut v = VolumeSpike::new("v", 3, dec!(2)).unwrap();
        v.update_bar(&bar_v("1000")).unwrap();
        v.update_bar(&bar_v("1000")).unwrap();
        // avg after 3 bars: (1000+1000+10000)/3 = 4000; ratio=10000/4000=2.5 >= 2 → spike
        v.update_bar(&bar_v("10000")).unwrap();
        assert!(v.is_spike());
    }

    #[test]
    fn test_vs_reset() {
        let mut v = VolumeSpike::new("v", 3, dec!(2)).unwrap();
        for _ in 0..5 { v.update_bar(&bar_v("1000")).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
        assert!(!v.is_spike());
    }
}
