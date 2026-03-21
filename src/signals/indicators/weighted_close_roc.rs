//! Weighted Close Rate of Change indicator.
//!
//! Computes the percentage rate of change of the weighted close price
//! `(high + low + 2 × close) / 4` over a rolling `period`, emphasizing the
//! close more heavily than the standard typical price.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rate of change of weighted close over `period` bars.
///
/// ```text
/// weighted_close = (high + low + 2 × close) / 4
/// roc = (wc[t] - wc[t-period]) / wc[t-period] × 100
/// ```
///
/// The weighted close formula gives twice the weight to the close versus the
/// high and low, making this ROC more sensitive to closing-price momentum.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// or when the base weighted close is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WeightedCloseRoc;
/// use fin_primitives::signals::Signal;
///
/// let wcr = WeightedCloseRoc::new("wcr", 10).unwrap();
/// assert_eq!(wcr.period(), 10);
/// assert!(!wcr.is_ready());
/// ```
pub struct WeightedCloseRoc {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl WeightedCloseRoc {
    /// Constructs a new `WeightedCloseRoc`.
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
            window: VecDeque::with_capacity(period + 1),
        })
    }
}

impl crate::signals::Signal for WeightedCloseRoc {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.window.len() > self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let wc = bar.weighted_close();
        self.window.push_back(wc);

        if self.window.len() > self.period + 1 {
            self.window.pop_front();
        }

        if self.window.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        let base = self.window[0];
        if base.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let roc = (wc - base)
            .checked_div(base)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(roc))
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

    fn bar(high: &str, low: &str, close: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(low.parse().unwrap()).unwrap(),
            high: Price::new(high.parse().unwrap()).unwrap(),
            low: Price::new(low.parse().unwrap()).unwrap(),
            close: Price::new(close.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_wcr_invalid_period() {
        assert!(WeightedCloseRoc::new("wcr", 0).is_err());
    }

    #[test]
    fn test_wcr_unavailable_during_warmup() {
        let mut wcr = WeightedCloseRoc::new("wcr", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(wcr.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_wcr_no_change_returns_zero() {
        let mut wcr = WeightedCloseRoc::new("wcr", 3).unwrap();
        for _ in 0..4 {
            wcr.update_bar(&bar("110", "90", "100")).unwrap();
        }
        let v = wcr.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_wcr_positive_roc_period_one() {
        let mut wcr = WeightedCloseRoc::new("wcr", 1).unwrap();
        // bar1: wc = (110 + 90 + 200) / 4 = 100
        wcr.update_bar(&bar("110", "90", "100")).unwrap();
        // bar2: wc = (120 + 100 + 220) / 4 = 110 → ROC = (110-100)/100*100 = 10
        let v = wcr.update_bar(&bar("120", "100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_wcr_reset() {
        let mut wcr = WeightedCloseRoc::new("wcr", 3).unwrap();
        for _ in 0..5 {
            wcr.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(wcr.is_ready());
        wcr.reset();
        assert!(!wcr.is_ready());
    }
}
