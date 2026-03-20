//! Close-to-High Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `close / high`.
///
/// Measures how consistently price closes near the session high.
/// Values near 1.0: closes at or near the high (bullish).
/// Values far below 1.0: closes significantly below the high (bearish rejection).
/// Bars with zero high are skipped.
pub struct CloseToHighRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseToHighRatio {
    /// Creates a new `CloseToHighRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseToHighRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if !bar.high.is_zero() {
            let ratio = bar.close / bar.high;
            self.window.push_back(ratio);
            self.sum += ratio;
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
    fn name(&self) -> &str { "CloseToHighRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: dec!(90),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_cthr_close_at_high() {
        // close = high → ratio = 1
        let mut sig = CloseToHighRatio::new(2).unwrap();
        sig.update(&bar("110", "110")).unwrap();
        let v = sig.update(&bar("110", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cthr_close_at_half_high() {
        // high=110, close=55 → ratio = 0.5
        let mut sig = CloseToHighRatio::new(2).unwrap();
        sig.update(&bar("110", "55")).unwrap();
        let v = sig.update(&bar("110", "55")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }
}
