//! Volume Swing indicator.
//!
//! Tracks the net balance between up-bar volume and down-bar volume over a
//! rolling window, revealing whether volume is flowing into bullish or bearish bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Swing: `sum(up_volume, N) - sum(down_volume, N)`.
///
/// For each bar:
/// - **Up bar** (`close > open`): the bar's volume is added to up-volume.
/// - **Down bar** (`close < open`): the bar's volume is added to down-volume.
/// - **Doji** (`close == open`): volume is ignored (contributes zero to both).
///
/// The rolling window sums `N` bars and returns the net: positive values indicate
/// buyers dominate by volume; negative values indicate sellers dominate.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeSwing;
/// use fin_primitives::signals::Signal;
///
/// let vs = VolumeSwing::new("vol_swing", 10).unwrap();
/// assert_eq!(vs.period(), 10);
/// assert!(!vs.is_ready());
/// ```
pub struct VolumeSwing {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeSwing {
    /// Constructs a new `VolumeSwing`.
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
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeSwing {
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
        let signed_vol = if bar.close > bar.open {
            bar.volume
        } else if bar.close < bar.open {
            -bar.volume
        } else {
            Decimal::ZERO
        };

        self.sum += signed_vol;
        self.window.push_back(signed_vol);

        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        Ok(SignalValue::Scalar(self.sum))
    }

    fn reset(&mut self) {
        self.window.clear();
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

    fn bar(open: &str, close: &str, vol: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let high = if c >= o { c } else { o };
        let low = if c <= o { c } else { o };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o, high, low, close: c,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vs_invalid_period() {
        assert!(VolumeSwing::new("vs", 0).is_err());
    }

    #[test]
    fn test_vs_unavailable_during_warmup() {
        let mut vs = VolumeSwing::new("vs", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(vs.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vs_all_up_bars_positive() {
        let mut vs = VolumeSwing::new("vs", 3).unwrap();
        for _ in 0..3 {
            vs.update_bar(&bar("100", "105", "1000")).unwrap();
        }
        let v = vs.update_bar(&bar("100", "105", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(3000)));
    }

    #[test]
    fn test_vs_all_down_bars_negative() {
        let mut vs = VolumeSwing::new("vs", 3).unwrap();
        for _ in 0..3 {
            vs.update_bar(&bar("105", "100", "1000")).unwrap();
        }
        let v = vs.update_bar(&bar("105", "100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-3000)));
    }

    #[test]
    fn test_vs_doji_zero_contribution() {
        let mut vs = VolumeSwing::new("vs", 2).unwrap();
        vs.update_bar(&bar("100", "100", "5000")).unwrap();
        // Only doji bars → all zeros
        let v = vs.update_bar(&bar("100", "100", "5000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vs_mixed_bars() {
        let mut vs = VolumeSwing::new("vs", 2).unwrap();
        vs.update_bar(&bar("100", "105", "1000")).unwrap(); // up: +1000
        let v = vs.update_bar(&bar("105", "100", "600")).unwrap();  // down: -600
        assert_eq!(v, SignalValue::Scalar(dec!(400)));
    }

    #[test]
    fn test_vs_reset() {
        let mut vs = VolumeSwing::new("vs", 3).unwrap();
        for _ in 0..3 {
            vs.update_bar(&bar("100", "105", "1000")).unwrap();
        }
        assert!(vs.is_ready());
        vs.reset();
        assert!(!vs.is_ready());
    }
}
