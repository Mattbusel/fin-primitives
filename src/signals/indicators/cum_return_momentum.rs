//! Cumulative Return Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// N-bar cumulative return: `close[t] / close[t-N] - 1`.
///
/// Measures the raw return over the look-back period without smoothing.
/// Equivalent to the raw momentum denominator used in many academic studies.
pub struct CumReturnMomentum {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl CumReturnMomentum {
    /// Creates a new `CumReturnMomentum` with the given N-bar look-back.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(period + 1) })
    }
}

impl Signal for CumReturnMomentum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        let base = *self.closes.front().unwrap();
        let current = *self.closes.back().unwrap();
        if base.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(current / base - Decimal::ONE))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "CumReturnMomentum" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_crm_flat_zero() {
        let mut sig = CumReturnMomentum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_crm_ten_percent_return() {
        // 100 → 110 over 2 bars → 10% return
        let mut sig = CumReturnMomentum::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("105")).unwrap();
        let v = sig.update(&bar("110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.1)));
    }
}
