//! Chaikin Money Flow (CMF) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Chaikin Money Flow (CMF) over a rolling `period`.
///
/// CMF measures buying/selling pressure by combining close position within
/// the bar range with volume:
///
/// ```text
/// MFM = ((close - low) - (high - close)) / (high - low)
/// MFV = MFM × volume
/// CMF = Σ(MFV, period) / Σ(volume, period)
/// ```
///
/// When `high == low` (zero range), MFM is treated as 0.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
/// Range is `[-1, 1]`: positive → buying pressure, negative → selling pressure.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Cmf;
/// use fin_primitives::signals::Signal;
/// use fin_primitives::signals::BarInput;
/// use rust_decimal_macros::dec;
///
/// let mut cmf = Cmf::new("cmf3", 3).unwrap();
/// // BarInput::new(close, high, low, open, volume)
/// cmf.update(&BarInput::new(dec!(105), dec!(110), dec!(90), dec!(100), dec!(1000))).unwrap();
/// ```
pub struct Cmf {
    name: String,
    period: usize,
    mfv_window: VecDeque<Decimal>,
    vol_window: VecDeque<Decimal>,
}

impl Cmf {
    /// Constructs a new `Cmf` indicator with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period` is zero.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            mfv_window: VecDeque::with_capacity(period),
            vol_window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Cmf {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let mfm = if range.is_zero() {
            Decimal::ZERO
        } else {
            ((bar.close - bar.low) - (bar.high - bar.close)) / range
        };
        let mfv = mfm * bar.volume;

        self.mfv_window.push_back(mfv);
        self.vol_window.push_back(bar.volume);
        if self.mfv_window.len() > self.period {
            self.mfv_window.pop_front();
            self.vol_window.pop_front();
        }

        if self.mfv_window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum_mfv: Decimal = self.mfv_window.iter().sum();
        let sum_vol: Decimal = self.vol_window.iter().sum();
        if sum_vol.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar(sum_mfv / sum_vol))
    }

    fn is_ready(&self) -> bool {
        self.mfv_window.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.mfv_window.clear();
        self.vol_window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;
    use rust_decimal_macros::dec;

    fn bar(open: &str, high: &str, low: &str, close: &str, vol: &str) -> BarInput {
        // BarInput::new signature: (close, high, low, open, volume)
        BarInput::new(
            close.parse().unwrap(),
            high.parse().unwrap(),
            low.parse().unwrap(),
            open.parse().unwrap(),
            vol.parse().unwrap(),
        )
    }

    #[test]
    fn test_cmf_period_zero_error() {
        assert!(Cmf::new("cmf", 0).is_err());
    }

    #[test]
    fn test_cmf_unavailable_before_period() {
        let mut cmf = Cmf::new("cmf2", 2).unwrap();
        let result = cmf.update(&bar("100", "110", "90", "105", "1000")).unwrap();
        assert_eq!(result, SignalValue::Unavailable);
    }

    #[test]
    fn test_cmf_ready_after_period() {
        let mut cmf = Cmf::new("cmf2", 2).unwrap();
        cmf.update(&bar("100", "110", "90", "105", "1000")).unwrap();
        let result = cmf.update(&bar("105", "115", "95", "110", "1200")).unwrap();
        assert!(matches!(result, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_cmf_zero_range_bar_contributes_zero_mfv() {
        let mut cmf = Cmf::new("cmf1", 1).unwrap();
        // high == low, so MFM = 0, MFV = 0
        let result = cmf.update(&bar("100", "100", "100", "100", "1000")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cmf_full_buying_pressure() {
        // close == high → MFM = 1, CMF = 1
        let mut cmf = Cmf::new("cmf1", 1).unwrap();
        let result = cmf.update(&bar("100", "110", "90", "110", "500")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cmf_full_selling_pressure() {
        // close == low → MFM = -1, CMF = -1
        let mut cmf = Cmf::new("cmf1", 1).unwrap();
        let result = cmf.update(&bar("100", "110", "90", "90", "500")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_cmf_reset_clears_state() {
        let mut cmf = Cmf::new("cmf2", 2).unwrap();
        cmf.update(&bar("100", "110", "90", "105", "1000")).unwrap();
        cmf.update(&bar("105", "115", "95", "110", "1200")).unwrap();
        cmf.reset();
        assert!(!cmf.is_ready());
        let result = cmf.update(&bar("100", "110", "90", "105", "1000")).unwrap();
        assert_eq!(result, SignalValue::Unavailable);
    }

    #[test]
    fn test_cmf_period_accessor() {
        let cmf = Cmf::new("cmf5", 5).unwrap();
        assert_eq!(cmf.period(), 5);
    }

    #[test]
    fn test_cmf_name_accessor() {
        let cmf = Cmf::new("my_cmf", 3).unwrap();
        assert_eq!(cmf.name(), "my_cmf");
    }
}
