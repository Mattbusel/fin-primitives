//! Volume Surge indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Surge — ratio of current volume to the median volume over the last
/// `period` bars.
///
/// A value > 1 indicates above-median volume (potential surge or institutional activity).
/// A value < 1 indicates below-median volume (quiet/consolidating).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// if median volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeSurge;
/// use fin_primitives::signals::Signal;
///
/// let vs = VolumeSurge::new("vs", 20).unwrap();
/// assert_eq!(vs.period(), 20);
/// ```
pub struct VolumeSurge {
    name: String,
    period: usize,
    volumes: VecDeque<Decimal>,
}

impl VolumeSurge {
    /// Constructs a new `VolumeSurge`.
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
        })
    }
}

impl Signal for VolumeSurge {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.volumes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.volumes.push_back(bar.volume);
        if self.volumes.len() > self.period {
            self.volumes.pop_front();
        }
        if self.volumes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mut sorted: Vec<Decimal> = self.volumes.iter().copied().collect();
        sorted.sort();
        let median = if self.period % 2 == 1 {
            sorted[self.period / 2]
        } else {
            let two = Decimal::TWO;
            (sorted[self.period / 2 - 1] + sorted[self.period / 2]) / two
        };

        if median.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(bar.volume / median))
    }

    fn reset(&mut self) {
        self.volumes.clear();
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
    fn test_vs_invalid_period() {
        assert!(VolumeSurge::new("vs", 0).is_err());
    }

    #[test]
    fn test_vs_unavailable_before_warm_up() {
        let mut vs = VolumeSurge::new("vs", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(vs.update_bar(&bar("1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vs_constant_volume_gives_one() {
        let mut vs = VolumeSurge::new("vs", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = vs.update_bar(&bar("1000")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_vs_spike_above_one() {
        let mut vs = VolumeSurge::new("vs", 3).unwrap();
        vs.update_bar(&bar("1000")).unwrap();
        vs.update_bar(&bar("1000")).unwrap();
        // 3rd bar: v=5000 → median([1000,1000,5000])=1000 → surge=5
        let result = vs.update_bar(&bar("5000")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_vs_reset() {
        let mut vs = VolumeSurge::new("vs", 3).unwrap();
        for _ in 0..3 { vs.update_bar(&bar("1000")).unwrap(); }
        assert!(vs.is_ready());
        vs.reset();
        assert!(!vs.is_ready());
    }
}
