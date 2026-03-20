//! Money Flow Index (MFI) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Money Flow Index (MFI) — a volume-weighted RSI variant.
///
/// MFI measures buying/selling pressure using both price and volume:
///
/// ```text
/// typical_price = (high + low + close) / 3
/// raw_money_flow = typical_price × volume
/// money_flow_ratio = sum(positive_MF, period) / sum(negative_MF, period)
/// MFI = 100 - 100 / (1 + money_flow_ratio)
/// ```
///
/// Positive money flow occurs when `typical_price > prev_typical_price`.
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Mfi;
/// use fin_primitives::signals::Signal;
/// use fin_primitives::signals::BarInput;
/// use rust_decimal_macros::dec;
///
/// let mut mfi = Mfi::new("mfi3", 3).unwrap();
/// ```
pub struct Mfi {
    name: String,
    period: usize,
    prev_tp: Option<Decimal>,
    pos_mf: VecDeque<Decimal>,
    neg_mf: VecDeque<Decimal>,
}

impl Mfi {
    /// Constructs a new `Mfi` indicator with the given name and period.
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
            prev_tp: None,
            pos_mf: VecDeque::with_capacity(period),
            neg_mf: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Mfi {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tp = bar.typical_price();
        let raw_mf = tp * bar.volume;

        if let Some(prev) = self.prev_tp {
            let (pos, neg) = if tp > prev {
                (raw_mf, Decimal::ZERO)
            } else if tp < prev {
                (Decimal::ZERO, raw_mf)
            } else {
                (Decimal::ZERO, Decimal::ZERO)
            };
            self.pos_mf.push_back(pos);
            self.neg_mf.push_back(neg);
            if self.pos_mf.len() > self.period {
                self.pos_mf.pop_front();
                self.neg_mf.pop_front();
            }
        }
        self.prev_tp = Some(tp);

        if self.pos_mf.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let pos_sum: Decimal = self.pos_mf.iter().sum();
        let neg_sum: Decimal = self.neg_mf.iter().sum();

        if neg_sum.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ONE_HUNDRED));
        }
        let ratio = pos_sum / neg_sum;
        let mfi = Decimal::ONE_HUNDRED - Decimal::ONE_HUNDRED / (Decimal::ONE + ratio);
        Ok(SignalValue::Scalar(mfi))
    }

    fn is_ready(&self) -> bool {
        self.pos_mf.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_tp = None;
        self.pos_mf.clear();
        self.neg_mf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;
    use rust_decimal_macros::dec;

    fn bar(close: &str, high: &str, low: &str, vol: &str) -> BarInput {
        // BarInput::new(close, high, low, open, volume)
        BarInput::new(
            close.parse().unwrap(),
            high.parse().unwrap(),
            low.parse().unwrap(),
            close.parse().unwrap(),
            vol.parse().unwrap(),
        )
    }

    #[test]
    fn test_mfi_period_zero_error() {
        assert!(Mfi::new("mfi", 0).is_err());
    }

    #[test]
    fn test_mfi_unavailable_before_period_plus_one() {
        let mut mfi = Mfi::new("mfi2", 2).unwrap();
        // First bar: no prev_tp yet, so no mf entry — still unavailable
        let r1 = mfi.update(&bar("105", "110", "100", "1000")).unwrap();
        assert_eq!(r1, SignalValue::Unavailable);
        // Second bar: 1 mf entry < period 2
        let r2 = mfi.update(&bar("106", "111", "101", "1000")).unwrap();
        assert_eq!(r2, SignalValue::Unavailable);
    }

    #[test]
    fn test_mfi_ready_after_period_plus_one_bars() {
        let mut mfi = Mfi::new("mfi2", 2).unwrap();
        mfi.update(&bar("100", "105", "95", "1000")).unwrap();
        mfi.update(&bar("105", "110", "100", "1000")).unwrap();
        // After 3rd bar we have 2 entries in window
        let r = mfi.update(&bar("102", "107", "97", "1000")).unwrap();
        assert!(matches!(r, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_mfi_all_positive_flow_returns_100() {
        let mut mfi = Mfi::new("mfi2", 2).unwrap();
        // Each bar raises typical price so all flow is positive
        mfi.update(&bar("100", "102", "98", "1000")).unwrap();
        mfi.update(&bar("102", "104", "100", "1000")).unwrap();
        mfi.update(&bar("104", "106", "102", "1000")).unwrap();
        let r = mfi.update(&bar("106", "108", "104", "1000")).unwrap();
        if let SignalValue::Scalar(v) = r {
            assert_eq!(v, dec!(100));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_mfi_reset_clears_state() {
        let mut mfi = Mfi::new("mfi2", 2).unwrap();
        mfi.update(&bar("100", "105", "95", "1000")).unwrap();
        mfi.update(&bar("105", "110", "100", "1000")).unwrap();
        mfi.update(&bar("102", "107", "97", "1000")).unwrap();
        mfi.reset();
        assert!(!mfi.is_ready());
        let r = mfi.update(&bar("100", "105", "95", "1000")).unwrap();
        assert_eq!(r, SignalValue::Unavailable);
    }

    #[test]
    fn test_mfi_period_accessor() {
        let mfi = Mfi::new("mfi5", 5).unwrap();
        assert_eq!(mfi.period(), 5);
    }

    #[test]
    fn test_mfi_name_accessor() {
        let mfi = Mfi::new("my_mfi", 3).unwrap();
        assert_eq!(mfi.name(), "my_mfi");
    }
}
