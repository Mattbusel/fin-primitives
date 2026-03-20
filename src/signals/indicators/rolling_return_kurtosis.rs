//! Rolling Return Kurtosis indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Excess kurtosis of close returns over the rolling period.
///
/// Measures tail heaviness relative to a normal distribution.
/// Positive (leptokurtic): fat tails, extreme returns more frequent.
/// Negative (platykurtic): thin tails, returns more uniformly distributed.
/// Requires at least 4 bars (period >= 4).
pub struct RollingReturnKurtosis {
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<f64>,
}

impl RollingReturnKurtosis {
    /// Creates a new `RollingReturnKurtosis` with the given period (min 4).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 4 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, returns: VecDeque::with_capacity(period) })
    }
}

impl Signal for RollingReturnKurtosis {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                if let (Some(c), Some(p)) = (bar.close.to_f64(), pc.to_f64()) {
                    let ret = (c - p) / p;
                    self.returns.push_back(ret);
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
        let vals: Vec<f64> = self.returns.iter().cloned().collect();
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        if var == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let m4 = vals.iter().map(|v| (v - mean).powi(4)).sum::<f64>() / n;
        let kurtosis = m4 / (var * var) - 3.0; // excess kurtosis

        match Decimal::from_f64_retain(kurtosis) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.returns.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.returns.clear(); }
    fn name(&self) -> &str { "RollingReturnKurtosis" }
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
    fn test_rrk_constant_prices_zero() {
        // Constant price → variance = 0 → kurtosis = 0
        let mut sig = RollingReturnKurtosis::new(4).unwrap();
        for _ in 0..5 {
            sig.update(&bar("100")).unwrap();
        }
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rrk_not_ready() {
        let mut sig = RollingReturnKurtosis::new(4).unwrap();
        for _ in 0..4 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }
}
