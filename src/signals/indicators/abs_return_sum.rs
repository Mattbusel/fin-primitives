//! Absolute Return Sum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling sum of absolute close-to-close returns.
///
/// `Σ |close[t] - close[t-1]|` over the rolling period.
///
/// Measures total price path length (activity) over the window.
/// High values: very active, choppy or trending market with large moves.
/// Low values: quiet market with small price movements.
pub struct AbsReturnSum {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl AbsReturnSum {
    /// Creates a new `AbsReturnSum` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for AbsReturnSum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let abs_ret = (bar.close - pc).abs();
            self.window.push_back(abs_ret);
            self.sum += abs_ret;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "AbsReturnSum" }
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
    fn test_ars_flat_zero() {
        // Constant prices → abs_ret = 0 each bar → sum = 0
        let mut sig = AbsReturnSum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ars_alternating() {
        // Up 5, down 5, up 5 → sum = 15
        let mut sig = AbsReturnSum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("105")).unwrap(); // +5
        sig.update(&bar("100")).unwrap(); // +5
        let v = sig.update(&bar("105")).unwrap(); // +5, sum=15
        assert_eq!(v, SignalValue::Scalar(dec!(15)));
    }
}
