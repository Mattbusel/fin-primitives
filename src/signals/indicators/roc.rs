//! Rate of Change (ROC) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rate of Change over `period` bars.
///
/// `ROC = (close - close[period]) / close[period] × 100`
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars have been seen.
/// Returns `Scalar(0)` when `close[period] == 0` to avoid division by zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Roc;
/// use fin_primitives::signals::Signal;
///
/// let mut roc = Roc::new("roc3", 3).unwrap();
/// ```
pub struct Roc {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl Roc {
    /// Constructs a new `Roc` with the given name and period.
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
            history: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for Roc {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        // Keep exactly period+1 values
        if self.history.len() > self.period + 1 {
            self.history.pop_front();
        }
        let oldest = self.history[0];
        if oldest.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let roc = (bar.close - oldest) / oldest * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(roc))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period + 1
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_roc_period_0_error() {
        assert!(Roc::new("r", 0).is_err());
    }

    #[test]
    fn test_roc_unavailable_before_period_plus_1() {
        let mut roc = Roc::new("roc3", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(roc.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!roc.is_ready());
    }

    #[test]
    fn test_roc_known_value() {
        // ROC(3): after [100, 110, 120, 130]: oldest=100, close=130
        // ROC = (130 - 100) / 100 * 100 = 30
        let mut roc = Roc::new("roc3", 3).unwrap();
        for p in &["100", "110", "120"] {
            roc.update_bar(&bar(p)).unwrap();
        }
        let v = roc.update_bar(&bar("130")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(30)));
    }

    #[test]
    fn test_roc_constant_prices_is_zero() {
        let mut roc = Roc::new("roc3", 3).unwrap();
        for _ in 0..3 {
            roc.update_bar(&bar("100")).unwrap();
        }
        let v = roc.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_roc_reset() {
        let mut roc = Roc::new("roc2", 2).unwrap();
        for _ in 0..3 {
            roc.update_bar(&bar("100")).unwrap();
        }
        assert!(roc.is_ready());
        roc.reset();
        assert!(!roc.is_ready());
        assert_eq!(roc.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
