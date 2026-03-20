//! Volatility Adjusted Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// N-bar return divided by the rolling standard deviation of returns.
///
/// `(close[t] / close[t-N] - 1) / std_dev(returns, N)`
///
/// Normalizes momentum by recent volatility — similar to a Sharpe-like ratio.
/// Returns 0 when standard deviation is zero (flat price series).
pub struct VolatilityAdjustedMomentum {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl VolatilityAdjustedMomentum {
    /// Creates a new `VolatilityAdjustedMomentum` with the given period (min 2).
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

        let base = *self.closes.front().unwrap();
        let current = *self.closes.back().unwrap();
        if base.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        // Compute N-bar return
        let n_bar_ret = (current / base - Decimal::ONE).to_f64().unwrap_or(0.0);

        // Compute returns in window
        let vals: Vec<f64> = self.closes.iter()
            .zip(self.closes.iter().skip(1))
            .filter_map(|(a, b)| {
                if a.is_zero() { return None; }
                (*b / *a - Decimal::ONE).to_f64()
            })
            .collect();

        if vals.len() < 2 {
            return Ok(SignalValue::Unavailable);
        }

        let n = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
        let std_dev = var.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let vam = n_bar_ret / std_dev;
        match Decimal::from_f64_retain(vam) {
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
    fn test_vam_flat_zero() {
        // Constant prices → std_dev=0 → result=0
        let mut sig = VolatilityAdjustedMomentum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vam_positive_momentum() {
        // Rising prices → positive VAM
        let mut sig = VolatilityAdjustedMomentum::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("103")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("106")).unwrap() {
            assert!(v > dec!(0), "expected positive VAM, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
