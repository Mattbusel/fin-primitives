//! Relative Volume Rank — percentile rank of current bar volume vs prior N bar volumes.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Relative Volume Rank — percentile rank of current volume within the last `period` bars.
///
/// Output in `[0, 1]`:
/// - **1.0**: highest volume in the window (volume spike / surge).
/// - **0.0**: lowest volume (near-zero or dull session).
/// - **0.5**: median volume.
///
/// Uses `count(past_vol < current_vol) / (period - 1)`.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RelativeVolumeRank;
/// use fin_primitives::signals::Signal;
/// let rvr = RelativeVolumeRank::new("rvr_14", 14).unwrap();
/// assert_eq!(rvr.period(), 14);
/// ```
pub struct RelativeVolumeRank {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl RelativeVolumeRank {
    /// Constructs a new `RelativeVolumeRank`.
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
        })
    }
}

impl Signal for RelativeVolumeRank {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let current = bar.volume;
        let count_below = self.window
            .iter()
            .take(self.period - 1) // exclude current bar
            .filter(|&&v| v < current)
            .count() as u32;

        let rank = Decimal::from(count_below)
            .checked_div(Decimal::from((self.period - 1) as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(rank.clamp(Decimal::ZERO, Decimal::ONE)))
    }

    fn reset(&mut self) {
        self.window.clear();
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
    fn test_rvr_invalid_period() {
        assert!(RelativeVolumeRank::new("rvr", 0).is_err());
        assert!(RelativeVolumeRank::new("rvr", 1).is_err());
    }

    #[test]
    fn test_rvr_unavailable_before_period() {
        let mut s = RelativeVolumeRank::new("rvr", 3).unwrap();
        assert_eq!(s.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("2000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rvr_highest_volume_gives_one() {
        let mut s = RelativeVolumeRank::new("rvr", 3).unwrap();
        s.update_bar(&bar("1000")).unwrap();
        s.update_bar(&bar("2000")).unwrap();
        let v = s.update_bar(&bar("5000")).unwrap(); // max in window → rank=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_rvr_lowest_volume_gives_zero() {
        let mut s = RelativeVolumeRank::new("rvr", 3).unwrap();
        s.update_bar(&bar("5000")).unwrap();
        s.update_bar(&bar("3000")).unwrap();
        let v = s.update_bar(&bar("500")).unwrap(); // min in window → rank=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rvr_output_in_unit_interval() {
        let mut s = RelativeVolumeRank::new("rvr", 4).unwrap();
        for vol in &["1000", "3000", "500", "2000", "4000"] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(vol)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_rvr_reset() {
        let mut s = RelativeVolumeRank::new("rvr", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("1000")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
