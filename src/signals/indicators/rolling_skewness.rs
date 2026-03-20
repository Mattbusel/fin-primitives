//! Rolling Skewness indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Skewness of close returns over the rolling period.
///
/// Positive skew: long right tail (more frequent large positive returns).
/// Negative skew: long left tail (more frequent large negative returns).
/// Normal distribution has skewness ≈ 0.
/// Requires at least 3 bars (period >= 3).
pub struct RollingSkewness {
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<f64>,
}

impl RollingSkewness {
    /// Creates a new `RollingSkewness` with the given period (min 3).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 3 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, returns: VecDeque::with_capacity(period) })
    }
}

impl Signal for RollingSkewness {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                if let (Some(c), Some(p)) = (bar.close.to_f64(), pc.to_f64()) {
                    self.returns.push_back((c - p) / p);
                    if self.returns.len() > self.period {
                        self.returns.pop_front();
                    }
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let mean = self.returns.iter().sum::<f64>() / n;
        let var = self.returns.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let std_dev = var.sqrt();
        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let m3 = self.returns.iter().map(|v| (v - mean).powi(3)).sum::<f64>() / n;
        let skewness = m3 / (std_dev * std_dev * std_dev);

        match Decimal::from_f64_retain(skewness) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.returns.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.returns.clear(); }
    fn name(&self) -> &str { "RollingSkewness" }
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
    fn test_rs_flat_zero() {
        // Constant prices → std_dev=0 → skewness=0
        let mut sig = RollingSkewness::new(3).unwrap();
        for _ in 0..5 { sig.update(&bar("100")).unwrap(); }
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rs_not_ready() {
        let mut sig = RollingSkewness::new(4).unwrap();
        for _ in 0..4 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }
}
