//! Open Gap Size indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `|open - prev_close| / prev_close * 100`.
///
/// Measures the average magnitude of opening gaps (overnight moves).
/// Does not distinguish direction — use `CloseToOpenReturn` for directional gaps.
pub struct OpenGapSize {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenGapSize {
    /// Creates a new `OpenGapSize` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenGapSize {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let gap = (bar.open - pc).abs() / pc * Decimal::ONE_HUNDRED;
                self.window.push_back(gap);
                self.sum += gap;
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
    fn name(&self) -> &str { "OpenGapSize" }
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
    fn test_open_gap_size_no_gap() {
        let mut sig = OpenGapSize::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap(); // seeds prev_close=100
        sig.update(&bar("100", "100")).unwrap(); // gap=0
        let v = sig.update(&bar("100", "100")).unwrap(); // gap=0, avg=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_open_gap_size_symmetric() {
        let mut sig = OpenGapSize::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap(); // seeds prev_close=100
        sig.update(&bar("102", "100")).unwrap(); // gap=2%, seeds prev_close=100
        let v = sig.update(&bar("98", "100")).unwrap();  // gap=2%, avg=2%
        if let SignalValue::Scalar(x) = v {
            assert!((x - dec!(2)).abs() < dec!(0.001), "avg gap should be 2%, got {}", x);
        }
    }
}
