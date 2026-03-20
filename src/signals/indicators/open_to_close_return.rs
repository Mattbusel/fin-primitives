//! Open-to-Close Return indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(close - open) / open * 100`.
///
/// Measures the average intra-bar return (close vs open).
/// Positive = bullish intra-bar move on average; negative = bearish.
/// Excludes bars where open is zero.
pub struct OpenToCloseReturn {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenToCloseReturn {
    /// Creates a new `OpenToCloseReturn` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenToCloseReturn {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if !bar.open.is_zero() {
            let ret = (bar.close - bar.open) / bar.open * Decimal::ONE_HUNDRED;
            self.window.push_back(ret);
            self.sum += ret;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "OpenToCloseReturn" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: dec!(200),
            low: dec!(1),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_otcr_flat_zero() {
        let mut sig = OpenToCloseReturn::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        let v = sig.update(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_otcr_bullish() {
        // open 100, close 110 → +10%
        let mut sig = OpenToCloseReturn::new(2).unwrap();
        sig.update(&bar("100", "110")).unwrap();
        let v = sig.update(&bar("100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
