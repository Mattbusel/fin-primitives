//! Volatility-Adjusted Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Momentum scaled by rolling volatility: `(close[t] - close[t-N]) / std_dev(returns)`.
///
/// Normalizes raw momentum by recent volatility so readings are comparable
/// across different market regimes and instruments.
pub struct VolatilityAdjustedMomentum {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl VolatilityAdjustedMomentum {
    /// Creates a new `VolatilityAdjustedMomentum` with the given period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(period + 1) })
    }
}

impl Signal for VolatilityAdjustedMomentum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        // Compute returns
        let returns: Vec<f64> = self.closes.iter()
            .zip(self.closes.iter().skip(1))
            .filter_map(|(a, b)| {
                if a.is_zero() { return None; }
                ((*b - *a) / *a).to_f64()
            })
            .collect();

        if returns.len() < 2 {
            return Ok(SignalValue::Unavailable);
        }

        let n = returns.len() as f64;
        let mean = returns.iter().sum::<f64>() / n;
        let var = returns.iter().map(|r| { let d = r - mean; d * d }).sum::<f64>() / (n - 1.0);
        let std_dev = var.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let oldest = *self.closes.front().unwrap();
        let newest = *self.closes.back().unwrap();
        let mom = match (newest - oldest).to_f64() {
            Some(m) => m,
            None => return Ok(SignalValue::Unavailable),
        };

        let adj = mom / std_dev;
        match Decimal::from_f64_retain(adj) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "VolatilityAdjustedMomentum" }
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
    fn test_vam_not_ready() {
        let mut sig = VolatilityAdjustedMomentum::new(4).unwrap();
        for _ in 0..4 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vam_constant_prices_zero() {
        // No momentum, no volatility => result = 0
        let mut sig = VolatilityAdjustedMomentum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
