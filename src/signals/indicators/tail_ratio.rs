//! Tail Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of upper wick length divided by lower wick length.
///
/// `upper_wick = high - max(open, close)`
/// `lower_wick = min(open, close) - low`
///
/// Values > 1: upper wicks dominate (selling pressure / rejection at highs).
/// Values < 1: lower wicks dominate (buying support / rejection at lows).
/// Bars where lower wick is zero are skipped.
pub struct TailRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl TailRatio {
    /// Creates a new `TailRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for TailRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let upper_wick = bar.upper_wick();
        let lower_wick = bar.lower_wick();

        if !lower_wick.is_zero() {
            let ratio = upper_wick / lower_wick;
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
    fn name(&self) -> &str { "TailRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_tail_ratio_equal_wicks() {
        // open=100, close=100, high=110, low=90 → upper=10, lower=10 → ratio=1
        let mut sig = TailRatio::new(2).unwrap();
        sig.update(&bar("100", "110", "90", "100")).unwrap();
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_tail_ratio_no_upper_wick() {
        // close=high, lower wick exists → ratio=0
        let mut sig = TailRatio::new(2).unwrap();
        sig.update(&bar("95", "110", "90", "110")).unwrap(); // upper=0, lower=5 → ratio=0
        let v = sig.update(&bar("95", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
