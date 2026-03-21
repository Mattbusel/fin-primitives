//! Range Pressure Index indicator.
//!
//! Combines the close's position within its range with volume to produce a
//! volume-weighted measure of directional pressure.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Pressure Index: `sum(clv * volume, N) / sum(volume, N)`.
///
/// The Close Location Value (CLV) measures where the close sits within the
/// bar's range (−1 at the low, +1 at the high). Multiplying by volume weights
/// bars where more volume was traded. The ratio of sums gives a volume-weighted
/// directional pressure index in `[−1, +1]`:
///
/// ```text
/// clv = ((close - low) - (high - close)) / (high - low)   (0 for flat bars)
/// rpi = Σ(clv × volume) / Σ(volume)
/// ```
///
/// - **> 0**: volume is concentrated in bars closing near highs — buyers in control.
/// - **< 0**: volume concentrated near lows — sellers in control.
/// - Returns `Unavailable` when cumulative volume is zero.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangePressureIndex;
/// use fin_primitives::signals::Signal;
///
/// let rpi = RangePressureIndex::new("rpi", 14).unwrap();
/// assert_eq!(rpi.period(), 14);
/// assert!(!rpi.is_ready());
/// ```
pub struct RangePressureIndex {
    name: String,
    period: usize,
    clv_vol_window: VecDeque<Decimal>,
    vol_window: VecDeque<Decimal>,
    clv_vol_sum: Decimal,
    vol_sum: Decimal,
}

impl RangePressureIndex {
    /// Constructs a new `RangePressureIndex`.
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
            clv_vol_window: VecDeque::with_capacity(period),
            vol_window: VecDeque::with_capacity(period),
            clv_vol_sum: Decimal::ZERO,
            vol_sum: Decimal::ZERO,
        })
    }
}

impl Signal for RangePressureIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.vol_window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let clv = bar.close_location_value();
        let vol = bar.volume;
        let clv_vol = clv * vol;

        self.clv_vol_sum += clv_vol;
        self.vol_sum += vol;

        self.clv_vol_window.push_back(clv_vol);
        self.vol_window.push_back(vol);

        if self.clv_vol_window.len() > self.period {
            if let Some(old_cv) = self.clv_vol_window.pop_front() {
                self.clv_vol_sum -= old_cv;
            }
            if let Some(old_v) = self.vol_window.pop_front() {
                self.vol_sum -= old_v;
            }
        }

        if self.vol_window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.vol_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let rpi = self
            .clv_vol_sum
            .checked_div(self.vol_sum)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(rpi))
    }

    fn reset(&mut self) {
        self.clv_vol_window.clear();
        self.vol_window.clear();
        self.clv_vol_sum = Decimal::ZERO;
        self.vol_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(high: &str, low: &str, close: &str, vol: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(low.parse().unwrap()).unwrap(),
            high: Price::new(high.parse().unwrap()).unwrap(),
            low: Price::new(low.parse().unwrap()).unwrap(),
            close: Price::new(close.parse().unwrap()).unwrap(),
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rpi_invalid_period() {
        assert!(RangePressureIndex::new("rpi", 0).is_err());
    }

    #[test]
    fn test_rpi_unavailable_during_warmup() {
        let mut rpi = RangePressureIndex::new("rpi", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(rpi.update_bar(&bar("110", "90", "100", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rpi_all_closes_at_high_returns_one() {
        let mut rpi = RangePressureIndex::new("rpi", 3).unwrap();
        for _ in 0..3 {
            rpi.update_bar(&bar("110", "90", "110", "1000")).unwrap();
        }
        let v = rpi.update_bar(&bar("110", "90", "110", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_rpi_all_closes_at_low_returns_negative_one() {
        let mut rpi = RangePressureIndex::new("rpi", 3).unwrap();
        for _ in 0..3 {
            rpi.update_bar(&bar("110", "90", "90", "1000")).unwrap();
        }
        let v = rpi.update_bar(&bar("110", "90", "90", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_rpi_all_closes_at_mid_returns_zero() {
        let mut rpi = RangePressureIndex::new("rpi", 3).unwrap();
        for _ in 0..3 {
            rpi.update_bar(&bar("110", "90", "100", "1000")).unwrap();
        }
        let v = rpi.update_bar(&bar("110", "90", "100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rpi_zero_volume_unavailable() {
        let mut rpi = RangePressureIndex::new("rpi", 2).unwrap();
        rpi.update_bar(&bar("110", "90", "105", "0")).unwrap();
        let v = rpi.update_bar(&bar("110", "90", "105", "0")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rpi_reset() {
        let mut rpi = RangePressureIndex::new("rpi", 3).unwrap();
        for _ in 0..3 {
            rpi.update_bar(&bar("110", "90", "105", "1000")).unwrap();
        }
        assert!(rpi.is_ready());
        rpi.reset();
        assert!(!rpi.is_ready());
    }
}
