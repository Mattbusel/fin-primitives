//! Close-to-Open Gap indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of overnight gap: `(open - prev_close) / prev_close * 100`.
///
/// Positive values indicate upward overnight gaps on average.
/// Negative values indicate downward overnight gaps on average.
/// Skips bars where prev_close is zero.
pub struct CloseToOpenGap {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseToOpenGap {
    /// Creates a new `CloseToOpenGap` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseToOpenGap {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let gap = (bar.open - pc) / pc * Decimal::ONE_HUNDRED;
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
    fn name(&self) -> &str { "CloseToOpenGap" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: c.parse().unwrap(),
            low: o.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_ctog_no_gap() {
        // open = prev_close → gap = 0
        let mut sig = CloseToOpenGap::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        let v = sig.update(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ctog_upward_gap() {
        // prev_close=100, open=102 → gap=+2%
        let mut sig = CloseToOpenGap::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        sig.update(&bar("102", "102")).unwrap(); // gap=+2
        let v = sig.update(&bar("102", "102")).unwrap(); // gap=0 (open=prev_close=102)
        // window=[2, 0], avg=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }
}
