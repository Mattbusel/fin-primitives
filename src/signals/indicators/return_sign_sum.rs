//! Return Sign Sum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling sum of return signs: +1 (up bar), -1 (down bar), 0 (flat).
///
/// Also known as "directional bias" — measures the net number of up vs down moves.
/// Positive values indicate bullish dominance; negative values indicate bearish dominance.
pub struct ReturnSignSum {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<i8>,
    sum: i32,
}

impl ReturnSignSum {
    /// Creates a new `ReturnSignSum` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: 0 })
    }
}

impl Signal for ReturnSignSum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let sign: i8 = if bar.close > pc { 1 } else if bar.close < pc { -1 } else { 0 };
            self.window.push_back(sign);
            self.sum += sign as i32;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old as i32;
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(Decimal::from(self.sum)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = 0; }
    fn name(&self) -> &str { "ReturnSignSum" }
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
    fn test_return_sign_sum_all_up() {
        let mut sig = ReturnSignSum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap(); // window=[+1,+1,+1], sum=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_return_sign_sum_mixed() {
        let mut sig = ReturnSignSum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap(); // +1
        sig.update(&bar("100")).unwrap(); // -1
        let v = sig.update(&bar("101")).unwrap(); // +1, window=[+1,-1,+1], sum=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }
}
