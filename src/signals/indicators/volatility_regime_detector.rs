//! Volatility Regime Detector indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Ratio of short-term return std dev to long-term return std dev.
///
/// Values > 1 indicate a high-volatility regime (current vol > baseline).
/// Values < 1 indicate a low-volatility / quiet regime.
/// `short_period` must be less than `long_period`.
pub struct VolatilityRegimeDetector {
    short_period: usize,
    long_period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
}

impl VolatilityRegimeDetector {
    /// Creates a new `VolatilityRegimeDetector`.
    pub fn new(short_period: usize, long_period: usize) -> Result<Self, FinError> {
        if short_period < 2 || long_period <= short_period {
            return Err(FinError::InvalidPeriod(long_period));
        }
        Ok(Self {
            short_period,
            long_period,
            prev_close: None,
            returns: VecDeque::with_capacity(long_period),
        })
    }

    fn std_dev(vals: &[f64]) -> f64 {
        let n = vals.len() as f64;
        if n < 2.0 { return 0.0; }
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / (n - 1.0);
        var.sqrt()
    }
}

impl Signal for VolatilityRegimeDetector {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc) / pc;
                self.returns.push_back(ret);
                if self.returns.len() > self.long_period {
                    self.returns.pop_front();
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.returns.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        let all: Vec<f64> = self.returns.iter().filter_map(|r| r.to_f64()).collect();
        if all.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        let short_vals = &all[all.len() - self.short_period..];
        let long_std = Self::std_dev(&all);
        let short_std = Self::std_dev(short_vals);

        if long_std == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ONE));
        }

        let ratio = short_std / long_std;
        match Decimal::from_f64_retain(ratio) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.returns.len() >= self.long_period }
    fn period(&self) -> usize { self.long_period }
    fn reset(&mut self) { self.prev_close = None; self.returns.clear(); }
    fn name(&self) -> &str { "VolatilityRegimeDetector" }
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
    fn test_vrd_not_ready() {
        let mut sig = VolatilityRegimeDetector::new(3, 6).unwrap();
        for _ in 0..6 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vrd_constant_returns_one() {
        // Constant prices → all std devs = 0 → returns 1.0
        let mut sig = VolatilityRegimeDetector::new(3, 6).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..7 {
            last = sig.update(&bar("100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }
}
