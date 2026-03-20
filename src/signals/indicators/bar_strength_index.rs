//! Bar Strength Index indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(close - low) / (high - low) * 100`.
///
/// Measures how strongly price closes within its own bar range.
/// 100 = close at high (maximum bullish strength per bar).
/// 0 = close at low (maximum bearish weakness per bar).
/// Bars with zero range contribute 50 (neutral).
pub struct BarStrengthIndex {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl BarStrengthIndex {
    /// Creates a new `BarStrengthIndex` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for BarStrengthIndex {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let strength = if range.is_zero() {
            Decimal::from(50u32)
        } else {
            (bar.close - bar.low) / range * Decimal::ONE_HUNDRED
        };
        self.window.push_back(strength);
        self.sum += strength;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
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
    fn name(&self) -> &str { "BarStrengthIndex" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_bsi_close_at_high() {
        let mut sig = BarStrengthIndex::new(2).unwrap();
        sig.update(&bar("110", "90", "110")).unwrap(); // strength=100
        let v = sig.update(&bar("110", "90", "110")).unwrap(); // avg=100
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_bsi_close_at_low() {
        let mut sig = BarStrengthIndex::new(2).unwrap();
        sig.update(&bar("110", "90", "90")).unwrap(); // strength=0
        let v = sig.update(&bar("110", "90", "90")).unwrap(); // avg=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
