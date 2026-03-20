//! Coppock Curve indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Coppock Curve — WMA of `ROC(long_roc) + ROC(short_roc)`.
///
/// Originally designed for monthly charts (14, 11, 10 periods), it signals
/// long-term momentum turns. A rise from below zero is a buy signal.
///
/// Returns [`SignalValue::Unavailable`] until enough bars are accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Coppock;
/// use fin_primitives::signals::Signal;
///
/// let mut c = Coppock::new("coppock", 14, 11, 10).unwrap();
/// assert_eq!(c.period(), 14);
/// ```
pub struct Coppock {
    name: String,
    long_roc: usize,
    short_roc: usize,
    wma_period: usize,
    closes: VecDeque<Decimal>,
    roc_values: VecDeque<Decimal>,
}

impl Coppock {
    /// Constructs a new `Coppock`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is zero.
    pub fn new(name: impl Into<String>, long_roc: usize, short_roc: usize, wma_period: usize) -> Result<Self, FinError> {
        if long_roc == 0 || short_roc == 0 || wma_period == 0 {
            return Err(FinError::InvalidPeriod(0));
        }
        let history = long_roc + 1;
        Ok(Self {
            name: name.into(),
            long_roc,
            short_roc,
            wma_period,
            closes: VecDeque::with_capacity(history),
            roc_values: VecDeque::with_capacity(wma_period),
        })
    }
}

impl Signal for Coppock {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        let need = self.long_roc + 1;
        if self.closes.len() > need {
            self.closes.pop_front();
        }
        if self.closes.len() < need {
            return Ok(SignalValue::Unavailable);
        }

        let current = *self.closes.back().unwrap();
        let long_base = self.closes[0];
        let short_idx = self.closes.len().saturating_sub(self.short_roc + 1);
        let short_base = self.closes[short_idx];

        if long_base == Decimal::ZERO || short_base == Decimal::ZERO {
            return Ok(SignalValue::Unavailable);
        }

        let roc_sum = (current - long_base) / long_base * Decimal::ONE_HUNDRED
            + (current - short_base) / short_base * Decimal::ONE_HUNDRED;

        self.roc_values.push_back(roc_sum);
        if self.roc_values.len() > self.wma_period {
            self.roc_values.pop_front();
        }
        if self.roc_values.len() < self.wma_period {
            return Ok(SignalValue::Unavailable);
        }

        // WMA: weights 1,2,...,wma_period
        let n = self.wma_period;
        let denom = (n * (n + 1) / 2) as u64;
        let wma: Decimal = self.roc_values.iter().enumerate()
            .map(|(i, &v)| v * Decimal::from((i + 1) as u64))
            .sum::<Decimal>() / Decimal::from(denom);

        Ok(SignalValue::Scalar(wma))
    }

    fn is_ready(&self) -> bool {
        self.roc_values.len() >= self.wma_period
    }

    fn period(&self) -> usize {
        self.long_roc
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.roc_values.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
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
    fn test_coppock_zero_period_error() {
        assert!(Coppock::new("c", 0, 11, 10).is_err());
        assert!(Coppock::new("c", 14, 0, 10).is_err());
        assert!(Coppock::new("c", 14, 11, 0).is_err());
    }

    #[test]
    fn test_coppock_unavailable_before_warmup() {
        // long_roc=3, short_roc=2, wma=3 → needs 3+1+3-1 = 6 bars
        let mut c = Coppock::new("c", 3, 2, 3).unwrap();
        for _ in 0..5 {
            assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(c.update_bar(&bar("100")).unwrap().is_scalar());
    }

    #[test]
    fn test_coppock_flat_price_zero() {
        let mut c = Coppock::new("c", 3, 2, 3).unwrap();
        for _ in 0..10 { c.update_bar(&bar("100")).unwrap(); }
        match c.update_bar(&bar("100")).unwrap() {
            SignalValue::Scalar(v) => assert_eq!(v, rust_decimal_macros::dec!(0)),
            _ => panic!("expected scalar"),
        }
    }

    #[test]
    fn test_coppock_reset() {
        let mut c = Coppock::new("c", 3, 2, 3).unwrap();
        for _ in 0..10 { c.update_bar(&bar("100")).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
