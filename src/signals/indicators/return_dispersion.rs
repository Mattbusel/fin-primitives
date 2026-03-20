//! Return Dispersion indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Rolling std dev of close returns divided by rolling mean absolute return.
///
/// Measures how dispersed returns are relative to their typical magnitude.
/// Higher values indicate more erratic, less consistent return behaviour.
pub struct ReturnDispersion {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
}

impl ReturnDispersion {
    /// Creates a new `ReturnDispersion` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for ReturnDispersion {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc) / pc;
                self.window.push_back(ret);
                if self.window.len() > self.period {
                    self.window.pop_front();
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.window.len() as f64;
        let vals: Vec<f64> = self.window.iter()
            .filter_map(|r| r.to_f64())
            .collect();
        if vals.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mean = vals.iter().sum::<f64>() / n;
        let variance = vals.iter().map(|r| { let d = r - mean; d * d }).sum::<f64>() / (n - 1.0);
        let std_dev = variance.sqrt();
        let mean_abs = vals.iter().map(|r| r.abs()).sum::<f64>() / n;

        if mean_abs == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let disp = std_dev / mean_abs;
        match Decimal::from_f64_retain(disp) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); }
    fn name(&self) -> &str { "ReturnDispersion" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_return_dispersion_not_ready() {
        let mut sig = ReturnDispersion::new(3).unwrap();
        for _ in 0..3 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_return_dispersion_constant_returns() {
        // Constant 1% returns — std dev = 0, dispersion = 0
        let mut sig = ReturnDispersion::new(3).unwrap();
        let prices = ["100", "101", "102.01", "103.0301"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = sig.update(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(x) = last {
            assert!(x < dec!(0.001), "constant returns should have near-zero dispersion, got {}", x);
        }
    }
}
