//! Close-vs-Open Range indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(close - open) / (high - low)`.
///
/// Measures where the close sits relative to the bar's range, normalized by direction:
/// - +1.0 = close at high (full bullish body, no upper wick)
/// - -1.0 = close at low (full bearish body, no lower wick)
/// - 0.0 = close at midpoint
///
/// Bars with zero range are skipped.
pub struct CloseVsOpenRange {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseVsOpenRange {
    /// Creates a new `CloseVsOpenRange` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CloseVsOpenRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if !range.is_zero() {
            let ratio = (bar.close - bar.open) / range;
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
        let len = Decimal::from(self.window.len() as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "CloseVsOpenRange" }
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
    fn test_close_vs_open_range_full_bull() {
        // open=low, close=high → (close-open)/(high-low) = range/range = 1
        let mut sig = CloseVsOpenRange::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "110")).unwrap();
        let v = sig.update(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_close_vs_open_range_full_bear() {
        // open=high, close=low → (close-open)/(high-low) = -range/range = -1
        let mut sig = CloseVsOpenRange::new(2).unwrap();
        sig.update(&bar("110", "110", "90", "90")).unwrap();
        let v = sig.update(&bar("110", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }
}
