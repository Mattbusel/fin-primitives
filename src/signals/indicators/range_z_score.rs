//! Range Z-Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Z-score of the current bar's range relative to the rolling window.
///
/// `(range - mean_range) / std_dev_range` — measures how unusual the current
/// bar's volatility is. +2 means an unusually wide bar; -2 means unusually narrow.
pub struct RangeZScore {
    period: usize,
    window: VecDeque<Decimal>,
}

impl RangeZScore {
    /// Creates a new `RangeZScore` with the given rolling period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for RangeZScore {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.window.push_back(range);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let vals: Vec<f64> = self.window.iter()
            .filter_map(|r| r.to_f64())
            .collect();
        if vals.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / (n - 1.0);
        let std_dev = var.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let current = match range.to_f64() {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };
        let z = (current - mean) / std_dev;
        match Decimal::from_f64_retain(z) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "RangeZScore" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_range_z_score_not_ready() {
        let mut sig = RangeZScore::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_range_z_score_constant_zero() {
        // All same range → std_dev = 0 → z = 0
        let mut sig = RangeZScore::new(3).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
