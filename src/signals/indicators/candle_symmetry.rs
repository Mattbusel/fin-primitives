//! Candle Symmetry indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `min(upper_wick, lower_wick) / max(upper_wick, lower_wick)`.
///
/// Values near 1.0 indicate balanced wicks (symmetric candles).
/// Values near 0.0 indicate one-sided wick dominance.
/// Bars with both wicks zero are excluded.
pub struct CandleSymmetry {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CandleSymmetry {
    /// Creates a new `CandleSymmetry` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for CandleSymmetry {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body_top = bar.body_high();
        let body_bot = bar.body_low();
        let upper = bar.high - body_top;
        let lower = body_bot - bar.low;

        let max_wick = upper.max(lower);
        if !max_wick.is_zero() {
            let min_wick = upper.min(lower);
            let sym = min_wick / max_wick;
            self.window.push_back(sym);
            self.sum += sym;
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
    fn name(&self) -> &str { "CandleSymmetry" }
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
    fn test_candle_symmetry_equal_wicks() {
        // Equal upper and lower wicks => symmetry = 1
        // open=100, close=100, high=110, low=90 => upper=10, lower=10
        let mut sig = CandleSymmetry::new(2).unwrap();
        sig.update(&bar("100", "110", "90", "100")).unwrap();
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_candle_symmetry_one_sided() {
        // Only upper wick: open=close=90, high=110, low=90 => upper=20, lower=0 => sym=0
        let mut sig = CandleSymmetry::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "90")).unwrap();
        let v = sig.update(&bar("90", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
