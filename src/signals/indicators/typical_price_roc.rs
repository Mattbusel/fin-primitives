//! Typical Price Rate of Change indicator.
//!
//! Computes the percentage rate of change of the typical price `(high + low + close) / 3`
//! over a rolling `period`, measuring momentum in terms of the bar's average price
//! rather than just the close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rate of change of typical price over `period` bars.
///
/// ```text
/// typical_price = (high + low + close) / 3
/// roc = (tp[t] - tp[t-period]) / tp[t-period] × 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (needs the current bar and the bar `period` steps ago). Returns
/// [`SignalValue::Unavailable`] if the base typical price is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TypicalPriceRoc;
/// use fin_primitives::signals::Signal;
///
/// let tpr = TypicalPriceRoc::new("tpr", 10).unwrap();
/// assert_eq!(tpr.period(), 10);
/// assert!(!tpr.is_ready());
/// ```
pub struct TypicalPriceRoc {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl TypicalPriceRoc {
    /// Constructs a new `TypicalPriceRoc`.
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

impl crate::signals::Signal for TypicalPriceRoc {
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
        let tp = bar.typical_price();
        self.window.push_back(tp);

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

        let roc = (tp - base)
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
    fn test_tpr_invalid_period() {
        assert!(TypicalPriceRoc::new("tpr", 0).is_err());
    }

    #[test]
    fn test_tpr_unavailable_during_warmup() {
        let mut tpr = TypicalPriceRoc::new("tpr", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(tpr.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_tpr_zero_change_returns_zero() {
        // All bars identical TP → ROC = 0
        let mut tpr = TypicalPriceRoc::new("tpr", 3).unwrap();
        for _ in 0..4 {
            tpr.update_bar(&bar("110", "90", "100")).unwrap();
        }
        let v = tpr.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_tpr_positive_roc() {
        // TP goes from 100 to 110 → ROC = 10%
        let mut tpr = TypicalPriceRoc::new("tpr", 1).unwrap();
        // bar with TP=100: high=110, low=90, close=100 → (110+90+100)/3=100
        tpr.update_bar(&bar("110", "90", "100")).unwrap();
        // bar with TP=110: high=120, low=100, close=110 → (120+100+110)/3=110
        let v = tpr.update_bar(&bar("120", "100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_tpr_reset() {
        let mut tpr = TypicalPriceRoc::new("tpr", 3).unwrap();
        for _ in 0..5 {
            tpr.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(tpr.is_ready());
        tpr.reset();
        assert!(!tpr.is_ready());
    }
}
