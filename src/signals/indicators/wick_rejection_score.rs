//! Wick Rejection Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `max(upper_wick, lower_wick) / body`.
///
/// Measures how dominant wicks are relative to the candle body.
/// High values indicate strong price rejection and potential reversals.
/// Returns `Unavailable` when body is zero (doji), and excludes those bars.
pub struct WickRejectionScore {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl WickRejectionScore {
    /// Creates a new `WickRejectionScore` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for WickRejectionScore {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.body_size();
        if !body.is_zero() {
            let upper = bar.upper_wick();
            let lower = bar.lower_wick();
            let dom_wick = upper.max(lower);
            let score = dom_wick / body;
            self.window.push_back(score);
            self.sum += score;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.window.len() as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "WickRejectionScore" }
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
    fn test_wick_rejection_no_wicks() {
        // No wicks: open=low, close=high => dom_wick = 0, score = 0
        let mut sig = WickRejectionScore::new(2).unwrap();
        sig.update(&bar("100", "110", "100", "110")).unwrap();
        let v = sig.update(&bar("100", "110", "100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_wick_rejection_large_wick() {
        // Large upper wick: open=100, close=101, high=111 => upper_wick=10, body=1, score=10
        let mut sig = WickRejectionScore::new(2).unwrap();
        sig.update(&bar("100", "111", "99", "101")).unwrap();
        let v = sig.update(&bar("100", "111", "99", "101")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert!(x > dec!(1), "large wick should score > 1, got {}", x);
        }
    }
}
