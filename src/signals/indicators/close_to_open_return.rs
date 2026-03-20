//! Close-to-Open Return indicator -- rolling average gap-open return.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of (open - prev_close) / prev_close * 100.
/// Measures the average overnight / gap-open return over the period.
pub struct CloseToOpenReturn {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseToOpenReturn {
    /// Creates a new `CloseToOpenReturn` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(0));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseToOpenReturn {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.open - pc) / pc * Decimal::ONE_HUNDRED;
                self.window.push_back(ret);
                self.sum += ret;
                if self.window.len() > self.period {
                    if let Some(old) = self.window.pop_front() {
                        self.sum -= old;
                    }
                }
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "CloseToOpenReturn" }
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
    fn test_close_to_open_return_not_ready() {
        let mut sig = CloseToOpenReturn::new(3).unwrap();
        // First bar just seeds prev_close
        assert_eq!(sig.update(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_close_to_open_return_zero_gap() {
        let mut sig = CloseToOpenReturn::new(2).unwrap();
        // close 100, open 100 = 0% gap
        sig.update(&bar("100", "100")).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        let v = sig.update(&bar("100", "100")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert_eq!(x, dec!(0));
        } else {
            panic!("expected scalar");
        }
    }
}
