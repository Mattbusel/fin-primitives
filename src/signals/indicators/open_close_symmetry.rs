//! Open-Close Symmetry indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `1 - |close - open| / (high - low)`: wick dominance measure.
///
/// Complement of bar efficiency — measures how much of the bar's range is wicks vs. body.
/// 1.0: pure doji bars (entire range is wicks, no net close-to-open movement).
/// 0.0: full-body bars (close at one extreme, open at the other).
/// Returns Unavailable for bars where high == low.
pub struct OpenCloseSymmetry {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenCloseSymmetry {
    /// Creates a new `OpenCloseSymmetry` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenCloseSymmetry {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let body = (bar.close - bar.open).abs();
        let symmetry = Decimal::ONE - body / range;

        self.window.push_back(symmetry);
        self.sum += symmetry;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum / Decimal::from(self.period as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "OpenCloseSymmetry" }
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
    fn test_ocs_doji_gives_one() {
        // open=close → body=0 → symmetry=1
        let mut sig = OpenCloseSymmetry::new(2).unwrap();
        sig.update(&bar("100", "110", "90", "100")).unwrap();
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ocs_full_body_gives_zero() {
        // open at low, close at high → body=range → symmetry=0
        let mut sig = OpenCloseSymmetry::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "110")).unwrap();
        let v = sig.update(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
