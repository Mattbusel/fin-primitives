//! Up/Down Volume Ratio — ratio of bullish bar volume to bearish bar volume over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Up/Down Volume Ratio — `sum(up_volume) / sum(down_volume)` over the last `period` bars.
///
/// An up-bar is a bar where `close >= open`; a down-bar is where `close < open`.
///
/// - Values **> 1**: buying pressure dominates (more volume on up bars).
/// - Values **< 1**: selling pressure dominates (more volume on down bars).
/// - Values **= 1**: balanced volume.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or when
/// there is no down volume in the window (preventing division by zero).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpDownVolumeRatio;
/// use fin_primitives::signals::Signal;
/// let udvr = UpDownVolumeRatio::new("udvr", 20).unwrap();
/// assert_eq!(udvr.period(), 20);
/// ```
pub struct UpDownVolumeRatio {
    name: String,
    period: usize,
    window: VecDeque<(Decimal, bool)>, // (volume, is_up)
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
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for UpDownVolumeRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let is_up = bar.close >= bar.open;
        self.window.push_back((bar.volume, is_up));
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let (up_vol, down_vol) = self.window.iter().fold(
            (Decimal::ZERO, Decimal::ZERO),
            |(u, d), &(vol, is_up)| {
                if is_up { (u + vol, d) } else { (u, d + vol) }
            },
        );

        if down_vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = up_vol.checked_div(down_vol).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
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

    fn bar(open: &str, close: &str, vol: &str) -> OhlcvBar {
        let op = Price::new(open.parse().unwrap()).unwrap();
        let cp = Price::new(close.parse().unwrap()).unwrap();
        let hp = if cp.value() >= op.value() { cp } else { op };
        let lp = if cp.value() < op.value() { cp } else { op };
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
    fn test_udvr_invalid_period() {
        assert!(UpDownVolumeRatio::new("udvr", 0).is_err());
    }

    #[test]
    fn test_udvr_unavailable_before_period() {
        let mut udvr = UpDownVolumeRatio::new("udvr", 3).unwrap();
        assert_eq!(udvr.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(udvr.update_bar(&bar("105", "110", "1000")).unwrap(), SignalValue::Unavailable);
        assert!(!udvr.is_ready());
    }

    #[test]
    fn test_udvr_all_up_bars_unavailable() {
        // No down volume → Unavailable
        let mut udvr = UpDownVolumeRatio::new("udvr", 3).unwrap();
        for _ in 0..3 {
            udvr.update_bar(&bar("100", "105", "1000")).unwrap();
        }
        let v = udvr.update_bar(&bar("105", "110", "2000")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_udvr_equal_volumes_gives_one() {
        // Equal up and down volume → ratio = 1
        let mut udvr = UpDownVolumeRatio::new("udvr", 2).unwrap();
        udvr.update_bar(&bar("100", "105", "1000")).unwrap(); // up
        let v = udvr.update_bar(&bar("105", "100", "1000")).unwrap(); // down
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_udvr_double_up_volume() {
        // 2x up volume vs down → ratio = 2
        let mut udvr = UpDownVolumeRatio::new("udvr", 2).unwrap();
        udvr.update_bar(&bar("100", "105", "2000")).unwrap(); // up, 2000
        let v = udvr.update_bar(&bar("105", "100", "1000")).unwrap(); // down, 1000
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(2)).abs() < dec!(0.0001), "expected 2, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_udvr_reset() {
        let mut udvr = UpDownVolumeRatio::new("udvr", 2).unwrap();
        udvr.update_bar(&bar("100", "105", "1000")).unwrap();
        udvr.update_bar(&bar("105", "100", "1000")).unwrap();
        assert!(udvr.is_ready());
        udvr.reset();
        assert!(!udvr.is_ready());
    }

    #[test]
    fn test_udvr_period_and_name() {
        let udvr = UpDownVolumeRatio::new("my_udvr", 20).unwrap();
        assert_eq!(udvr.period(), 20);
        assert_eq!(udvr.name(), "my_udvr");
    }
}
