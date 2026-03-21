//! Volume Trend Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Trend Index (VTI).
///
/// Measures the directional bias of volume over a rolling window.
/// Up-bars (close >= open) contribute their volume to the "up" bucket;
/// down-bars contribute to the "down" bucket.
///
/// Formula: `vti = (up_volume - down_volume) / total_volume` ∈ [−1, +1]
///
/// - +1: all volume is on up-bars (strong bullish volume trend).
/// - −1: all volume is on down-bars (strong bearish volume trend).
/// - 0: balanced volume between up and down bars.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeTrendIndex;
/// use fin_primitives::signals::Signal;
/// let vti = VolumeTrendIndex::new("vti_14", 14).unwrap();
/// assert_eq!(vti.period(), 14);
/// ```
pub struct VolumeTrendIndex {
    name: String,
    period: usize,
    /// (up_volume, total_volume) per bar
    bars: VecDeque<(Decimal, Decimal)>,
}

impl VolumeTrendIndex {
    /// Constructs a new `VolumeTrendIndex`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, bars: VecDeque::with_capacity(period) })
    }
}

impl Signal for VolumeTrendIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let up_vol = if bar.close >= bar.open { bar.volume } else { Decimal::ZERO };
        self.bars.push_back((up_vol, bar.volume));
        if self.bars.len() > self.period {
            self.bars.pop_front();
        }
        if self.bars.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let up_sum: Decimal = self.bars.iter().map(|(u, _)| u).copied().sum();
        let total_sum: Decimal = self.bars.iter().map(|(_, t)| t).copied().sum();

        if total_sum.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let down_sum = total_sum - up_sum;
        let vti = (up_sum - down_sum)
            .checked_div(total_sum)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(vti))
    }

    fn is_ready(&self) -> bool {
        self.bars.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.bars.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str, vol: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let hi = op.max(cl);
        let lo = op.min(cl);
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hi, low: lo, close: cl,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(VolumeTrendIndex::new("vti", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut vti = VolumeTrendIndex::new("vti", 3).unwrap();
        assert_eq!(vti.update_bar(&bar("10", "11", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_all_up_gives_one() {
        let mut vti = VolumeTrendIndex::new("vti", 3).unwrap();
        for _ in 0..3 {
            vti.update_bar(&bar("10", "11", "1000")).unwrap();
        }
        let v = vti.update_bar(&bar("10", "11", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_all_down_gives_minus_one() {
        let mut vti = VolumeTrendIndex::new("vti", 3).unwrap();
        for _ in 0..3 {
            vti.update_bar(&bar("11", "10", "1000")).unwrap();
        }
        let v = vti.update_bar(&bar("11", "10", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_reset() {
        let mut vti = VolumeTrendIndex::new("vti", 2).unwrap();
        vti.update_bar(&bar("10", "11", "500")).unwrap();
        vti.update_bar(&bar("10", "11", "500")).unwrap();
        assert!(vti.is_ready());
        vti.reset();
        assert!(!vti.is_ready());
    }
}
