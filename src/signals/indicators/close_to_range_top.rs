//! Close-to-Range-Top indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(high - close) / (high - low) * 100`.
///
/// Measures how far the close is from the bar high as a percentage of total range.
/// Low values (near 0) indicate closes near the high (bullish strength).
/// High values (near 100) indicate closes near the low (bearish weakness).
pub struct CloseToRangeTop {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseToRangeTop {
    /// Creates a new `CloseToRangeTop` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseToRangeTop {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let ratio = if range.is_zero() {
            Decimal::ZERO
        } else {
            (bar.high - bar.close) / range * Decimal::ONE_HUNDRED
        };
        self.window.push_back(ratio);
        self.sum += ratio;
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
    fn name(&self) -> &str { "CloseToRangeTop" }
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
    fn test_close_to_range_top_at_high() {
        // close = high → (high-close)/range = 0
        let mut sig = CloseToRangeTop::new(2).unwrap();
        sig.update(&bar("110", "90", "110")).unwrap();
        let v = sig.update(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_close_to_range_top_at_low() {
        // close = low → (high-close)/range = 1 → 100%
        let mut sig = CloseToRangeTop::new(2).unwrap();
        sig.update(&bar("110", "90", "90")).unwrap();
        let v = sig.update(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }
}
