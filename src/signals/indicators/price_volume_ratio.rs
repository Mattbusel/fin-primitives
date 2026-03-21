//! Price-Volume Ratio indicator.
//!
//! Computes the rolling SMA of `close / volume`, measuring the price per unit
//! of volume. Rising values indicate each unit of volume is associated with
//! higher price levels; declining values indicate more volume per price unit.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling SMA of `close / volume`.
///
/// When volume is zero for a bar, that bar is skipped (no observation added).
/// Returns [`SignalValue::Unavailable`] until `period` non-zero-volume bars
/// have been accumulated.
///
/// High values indicate price is elevated relative to volume activity
/// (possible thin-market condition). Low values indicate heavy volume at
/// lower price levels.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceVolumeRatio;
/// use fin_primitives::signals::Signal;
///
/// let pvr = PriceVolumeRatio::new("pvr", 14).unwrap();
/// assert_eq!(pvr.period(), 14);
/// assert!(!pvr.is_ready());
/// ```
pub struct PriceVolumeRatio {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceVolumeRatio {
    /// Constructs a new `PriceVolumeRatio`.
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

impl crate::signals::Signal for PriceVolumeRatio {
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
        if bar.volume.is_zero() {
            // Skip zero-volume bars
            if self.window.len() < self.period {
                return Ok(SignalValue::Unavailable);
            }
            #[allow(clippy::cast_possible_truncation)]
            let mean = self.sum
                .checked_div(Decimal::from(self.period as u32))
                .ok_or(FinError::ArithmeticOverflow)?;
            return Ok(SignalValue::Scalar(mean));
        }

        let ratio = bar.close
            .checked_div(bar.volume)
            .ok_or(FinError::ArithmeticOverflow)?;

        self.sum += ratio;
        self.window.push_back(ratio);

        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        #[allow(clippy::cast_possible_truncation)]
        let mean = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(mean))
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

    fn bar(close: &str, vol: &str) -> OhlcvBar {
        let c = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: c, high: c, low: c, close: c,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pvr_invalid_period() {
        assert!(PriceVolumeRatio::new("pvr", 0).is_err());
    }

    #[test]
    fn test_pvr_unavailable_during_warmup() {
        let mut pvr = PriceVolumeRatio::new("pvr", 3).unwrap();
        assert_eq!(pvr.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(pvr.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pvr_constant_returns_ratio() {
        let mut pvr = PriceVolumeRatio::new("pvr", 3).unwrap();
        // close=100, vol=1000 → ratio=0.1 each bar
        for _ in 0..3 {
            pvr.update_bar(&bar("100", "1000")).unwrap();
        }
        let v = pvr.update_bar(&bar("100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.1)));
    }

    #[test]
    fn test_pvr_reset() {
        let mut pvr = PriceVolumeRatio::new("pvr", 3).unwrap();
        for _ in 0..3 {
            pvr.update_bar(&bar("100", "1000")).unwrap();
        }
        assert!(pvr.is_ready());
        pvr.reset();
        assert!(!pvr.is_ready());
    }
}
