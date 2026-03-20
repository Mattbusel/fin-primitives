//! Close-to-Mid-Range indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `close - (high + low) / 2`.
///
/// Measures whether the close tends to finish above or below the midpoint of the bar.
/// Positive = close bias toward high (bullish intra-bar momentum).
/// Negative = close bias toward low (bearish intra-bar momentum).
pub struct CloseToMidRange {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseToMidRange {
    /// Creates a new `CloseToMidRange` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseToMidRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = (bar.high + bar.low) / Decimal::TWO;
        let diff = bar.close - mid;
        self.window.push_back(diff);
        self.sum += diff;
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
    fn name(&self) -> &str { "CloseToMidRange" }
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
    fn test_close_to_mid_at_high() {
        // close = high = 110, mid = 100 → diff = 10
        let mut sig = CloseToMidRange::new(2).unwrap();
        sig.update(&bar("110", "90", "110")).unwrap();
        let v = sig.update(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_close_to_mid_at_mid() {
        // close = midpoint → diff = 0
        let mut sig = CloseToMidRange::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
