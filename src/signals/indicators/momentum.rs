//! Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Raw Momentum over `period` bars: `close - close[period]`.
///
/// Unlike [`crate::signals::indicators::Roc`], this is not normalised — it
/// returns the absolute price difference. Positive values indicate an upward
/// move; negative values a downward move.
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Momentum;
/// use fin_primitives::signals::Signal;
///
/// let mut mom = Momentum::new("mom5", 5).unwrap();
/// ```
pub struct Momentum {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl Momentum {
    /// Constructs a new `Momentum` with the given name and period.
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

impl Signal for Momentum {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        if self.history.len() > self.period + 1 {
            self.history.pop_front();
        }
        Ok(SignalValue::Scalar(bar.close - self.history[0]))
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
    fn test_momentum_period_0_error() {
        assert!(Momentum::new("m", 0).is_err());
    }

    #[test]
    fn test_momentum_known_value() {
        // Momentum(3): [100,110,120,150] → 150 - 100 = 50
        let mut mom = Momentum::new("mom3", 3).unwrap();
        for p in &["100", "110", "120"] {
            mom.update_bar(&bar(p)).unwrap();
        }
        let v = mom.update_bar(&bar("150")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_momentum_negative() {
        // Momentum(2): [100,90,80] → oldest=100, close=80, momentum = 80 - 100 = -20
        let mut mom = Momentum::new("mom2", 2).unwrap();
        mom.update_bar(&bar("100")).unwrap();
        mom.update_bar(&bar("90")).unwrap();
        let v = mom.update_bar(&bar("80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-20)));
    }

    #[test]
    fn test_momentum_constant_is_zero() {
        let mut mom = Momentum::new("mom3", 3).unwrap();
        for _ in 0..3 {
            mom.update_bar(&bar("50")).unwrap();
        }
        let v = mom.update_bar(&bar("50")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_momentum_reset() {
        let mut mom = Momentum::new("mom2", 2).unwrap();
        for _ in 0..3 {
            mom.update_bar(&bar("100")).unwrap();
        }
        assert!(mom.is_ready());
        mom.reset();
        assert!(!mom.is_ready());
    }
}
