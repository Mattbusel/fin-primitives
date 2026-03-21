//! Volume Ratio Trend indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Ratio Trend.
///
/// Compares the ratio of up-volume to total volume over a rolling window.
/// "Up-volume" is volume from bars where `close >= open`; "down-volume" is
/// from bars where `close < open`.
///
/// Formula: `vrt = up_volume / total_volume`
///
/// - 1.0: all volume in bullish bars.
/// - 0.0: all volume in bearish bars.
/// - 0.5: balanced.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
/// Returns `SignalValue::Scalar(0.5)` when total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeRatioTrend;
/// use fin_primitives::signals::Signal;
/// let vrt = VolumeRatioTrend::new("vrt_20", 20).unwrap();
/// assert_eq!(vrt.period(), 20);
/// ```
pub struct VolumeRatioTrend {
    name: String,
    period: usize,
    up_vols: VecDeque<Decimal>,
    all_vols: VecDeque<Decimal>,
}

impl VolumeRatioTrend {
    /// Constructs a new `VolumeRatioTrend`.
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
            up_vols: VecDeque::with_capacity(period),
            all_vols: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolumeRatioTrend {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let up_vol = if bar.close >= bar.open { bar.volume } else { Decimal::ZERO };
        self.up_vols.push_back(up_vol);
        self.all_vols.push_back(bar.volume);

        if self.up_vols.len() > self.period {
            self.up_vols.pop_front();
            self.all_vols.pop_front();
        }
        if self.up_vols.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let total_up: Decimal = self.up_vols.iter().copied().sum();
        let total_all: Decimal = self.all_vols.iter().copied().sum();

        if total_all.is_zero() {
            return Ok(SignalValue::Scalar(
                Decimal::from(5u32)
                    .checked_div(Decimal::from(10u32))
                    .ok_or(FinError::ArithmeticOverflow)?,
            ));
        }

        let ratio = total_up.checked_div(total_all).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool {
        self.up_vols.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.up_vols.clear();
        self.all_vols.clear();
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
        let hi = if op > cl { op } else { cl };
        let lo = if op < cl { op } else { cl };
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
        assert!(matches!(VolumeRatioTrend::new("vrt", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut vrt = VolumeRatioTrend::new("vrt", 3).unwrap();
        assert_eq!(vrt.update_bar(&bar("10", "12", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_all_bullish_gives_one() {
        let mut vrt = VolumeRatioTrend::new("vrt", 3).unwrap();
        for _ in 0..3 {
            vrt.update_bar(&bar("10", "12", "100")).unwrap();
        }
        let v = vrt.update_bar(&bar("10", "12", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_all_bearish_gives_zero() {
        let mut vrt = VolumeRatioTrend::new("vrt", 3).unwrap();
        for _ in 0..3 {
            vrt.update_bar(&bar("12", "10", "100")).unwrap();
        }
        let v = vrt.update_bar(&bar("12", "10", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut vrt = VolumeRatioTrend::new("vrt", 2).unwrap();
        vrt.update_bar(&bar("10", "12", "100")).unwrap();
        vrt.update_bar(&bar("10", "12", "100")).unwrap();
        assert!(vrt.is_ready());
        vrt.reset();
        assert!(!vrt.is_ready());
    }
}
