//! Volume Trend indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Trend — ratio of recent average volume to longer-term average volume.
///
/// ```text
/// short_avg = mean(volume, short_period)
/// long_avg  = mean(volume, long_period)
/// output    = short_avg / long_avg
/// ```
///
/// Values > 1 indicate rising volume participation; < 1 indicate declining volume.
/// Returns 1 when long_avg is zero.
///
/// Returns [`SignalValue::Unavailable`] until `long_period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeTrend;
/// use fin_primitives::signals::Signal;
///
/// let vt = VolumeTrend::new("vt", 5, 20).unwrap();
/// assert_eq!(vt.period(), 20);
/// ```
pub struct VolumeTrend {
    name: String,
    short: usize,
    long: usize,
    volumes: VecDeque<Decimal>,
}

impl VolumeTrend {
    /// Creates a new `VolumeTrend`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `short == 0`.
    /// Returns [`FinError::InvalidInput`] if `short >= long`.
    pub fn new(name: impl Into<String>, short: usize, long: usize) -> Result<Self, FinError> {
        if short == 0 { return Err(FinError::InvalidPeriod(short)); }
        if short >= long {
            return Err(FinError::InvalidInput("short must be less than long".into()));
        }
        Ok(Self {
            name: name.into(),
            short,
            long,
            volumes: VecDeque::with_capacity(long),
        })
    }
}

impl Signal for VolumeTrend {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.volumes.push_back(bar.volume);
        if self.volumes.len() > self.long { self.volumes.pop_front(); }
        if self.volumes.len() < self.long { return Ok(SignalValue::Unavailable); }

        let long_avg = self.volumes.iter().sum::<Decimal>() / Decimal::from(self.long as u32);

        let short_avg = self.volumes.iter().rev().take(self.short).sum::<Decimal>()
            / Decimal::from(self.short as u32);

        if long_avg.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ONE));
        }

        Ok(SignalValue::Scalar(short_avg / long_avg))
    }

    fn is_ready(&self) -> bool { self.volumes.len() >= self.long }
    fn period(&self) -> usize { self.long }

    fn reset(&mut self) {
        self.volumes.clear();
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
    fn test_vt_invalid() {
        assert!(VolumeTrend::new("v", 0, 10).is_err());
        assert!(VolumeTrend::new("v", 10, 5).is_err());
        assert!(VolumeTrend::new("v", 10, 10).is_err());
    }

    #[test]
    fn test_vt_unavailable_before_warmup() {
        let mut v = VolumeTrend::new("v", 3, 5).unwrap();
        for _ in 0..4 {
            assert_eq!(v.update_bar(&bar_v("1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vt_uniform_is_one() {
        // Equal volume throughout → short_avg = long_avg → ratio = 1
        let mut v = VolumeTrend::new("v", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..8 { last = v.update_bar(&bar_v("1000")).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vt_high_recent_volume_above_one() {
        // 5 low-volume bars, then 3 high-volume bars → short_avg > long_avg
        let mut v = VolumeTrend::new("v", 3, 5).unwrap();
        // Fill with 5 bars of 100 (long period)
        for _ in 0..5 { v.update_bar(&bar_v("100")).unwrap(); }
        // Push 3 high-volume bars (rolls out the low-volume ones gradually)
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 { last = v.update_bar(&bar_v("1000")).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            // long window has 2 bars of 100 and 3 of 1000; short window is 3 bars of 1000
            assert!(val > dec!(1), "expected > 1, got {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vt_reset() {
        let mut v = VolumeTrend::new("v", 3, 5).unwrap();
        for _ in 0..8 { v.update_bar(&bar_v("1000")).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
