//! High-Volume Bar Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Volume Bar Ratio — fraction of the last N bars where volume exceeds the rolling average.
///
/// ```text
/// avg_vol = mean(volume, N)
/// ratio   = count(volume_i > avg_vol) / N * 100
/// ```
///
/// - **High value**: many bars with above-average volume — active, high-participation market.
/// - **Low value**: mostly low-volume bars — thin, low-conviction trading.
/// - **Near 50%**: balanced volume distribution.
/// - Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighVolumeBarRatio;
/// use fin_primitives::signals::Signal;
///
/// let hvbr = HighVolumeBarRatio::new("hvbr", 20).unwrap();
/// assert_eq!(hvbr.period(), 20);
/// ```
pub struct HighVolumeBarRatio {
    name: String,
    period: usize,
    volumes: VecDeque<Decimal>,
    sum: Decimal,
}

impl HighVolumeBarRatio {
    /// Constructs a new `HighVolumeBarRatio`.
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

impl Signal for HighVolumeBarRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.volumes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let vol = bar.volume;

        self.sum += vol;
        self.volumes.push_back(vol);
        if self.volumes.len() > self.period {
            let removed = self.volumes.pop_front().unwrap();
            self.sum -= removed;
        }

        if self.volumes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let count = self.volumes.iter().filter(|&&v| v > avg).count();
        #[allow(clippy::cast_possible_truncation)]
        let pct = Decimal::from(count as u32)
            / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(pct))
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

    fn bar(vol: u64) -> OhlcvBar {
        let p = Price::new("100".parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::new(Decimal::from(vol)).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_hvbr_invalid_period() {
        assert!(HighVolumeBarRatio::new("hvbr", 0).is_err());
    }

    #[test]
    fn test_hvbr_unavailable_during_warmup() {
        let mut hvbr = HighVolumeBarRatio::new("hvbr", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(hvbr.update_bar(&bar(1000)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!hvbr.is_ready());
    }

    #[test]
    fn test_hvbr_uniform_volume_zero() {
        // All equal volumes → avg = vol → 0 bars strictly above avg → 0%
        let mut hvbr = HighVolumeBarRatio::new("hvbr", 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..4 {
            last = hvbr.update_bar(&bar(1000)).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hvbr_two_high_two_low() {
        // 2 high vol + 2 low vol → avg = 550 → count > avg = 2 → 50%
        let mut hvbr = HighVolumeBarRatio::new("hvbr", 4).unwrap();
        hvbr.update_bar(&bar(1000)).unwrap();
        hvbr.update_bar(&bar(1000)).unwrap();
        hvbr.update_bar(&bar(100)).unwrap();
        let last = hvbr.update_bar(&bar(100)).unwrap();
        assert_eq!(last, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_hvbr_reset() {
        let mut hvbr = HighVolumeBarRatio::new("hvbr", 3).unwrap();
        for _ in 0..3 { hvbr.update_bar(&bar(1000)).unwrap(); }
        assert!(hvbr.is_ready());
        hvbr.reset();
        assert!(!hvbr.is_ready());
    }
}
