//! Up/Down Volume Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Up/Down Volume Ratio — the ratio of total up-bar volume to total down-bar
/// volume over `period` bars.
///
/// - `> 1` → buying pressure dominates  
/// - `< 1` → selling pressure dominates  
/// - Returns [`SignalValue::Unavailable`] if down volume is zero or fewer than `period` bars.
///
/// A bar is an "up-bar" if `close > open`, and a "down-bar" if `close < open`.
/// Doji bars (close == open) contribute to neither side.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpDownVolumeRatio;
/// use fin_primitives::signals::Signal;
///
/// let uvr = UpDownVolumeRatio::new("uvr", 20).unwrap();
/// assert_eq!(uvr.period(), 20);
/// ```
pub struct UpDownVolumeRatio {
    name: String,
    period: usize,
    up_vols: VecDeque<Decimal>,
    dn_vols: VecDeque<Decimal>,
}

impl UpDownVolumeRatio {
    /// Constructs a new `UpDownVolumeRatio`.
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
            dn_vols: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for UpDownVolumeRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.up_vols.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let (up, dn) = if bar.is_bullish() {
            (bar.volume, Decimal::ZERO)
        } else if bar.is_bearish() {
            (Decimal::ZERO, bar.volume)
        } else {
            (Decimal::ZERO, Decimal::ZERO)
        };

        self.up_vols.push_back(up);
        self.dn_vols.push_back(dn);
        if self.up_vols.len() > self.period { self.up_vols.pop_front(); }
        if self.dn_vols.len() > self.period { self.dn_vols.pop_front(); }

        if self.up_vols.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let up_sum: Decimal = self.up_vols.iter().sum();
        let dn_sum: Decimal = self.dn_vols.iter().sum();

        if dn_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(up_sum / dn_sum))
    }

    fn reset(&mut self) {
        self.up_vols.clear();
        self.dn_vols.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str, v: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let vq = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp, low: op, close: cp,
            volume: vq,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_uvr_invalid_period() {
        assert!(UpDownVolumeRatio::new("uvr", 0).is_err());
    }

    #[test]
    fn test_uvr_unavailable_before_warm_up() {
        let mut uvr = UpDownVolumeRatio::new("uvr", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(uvr.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_uvr_equal_up_down() {
        // 2 up bars, 2 down bars, same volume → ratio ≈ 1
        let mut uvr = UpDownVolumeRatio::new("uvr", 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..2 {
            uvr.update_bar(&bar("100", "105", "1000")).unwrap();
            last = uvr.update_bar(&bar("105", "100", "1000")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_uvr_reset() {
        let mut uvr = UpDownVolumeRatio::new("uvr", 3).unwrap();
        for _ in 0..3 { uvr.update_bar(&bar("100", "105", "1000")).unwrap(); }
        assert!(uvr.is_ready());
        uvr.reset();
        assert!(!uvr.is_ready());
    }
}
