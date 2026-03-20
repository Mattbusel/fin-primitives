//! Volume Trend Slope indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Linear regression slope of volume over the rolling period.
///
/// Positive slope: volume trending upward (growing interest).
/// Negative slope: volume trending downward (fading interest).
/// Uses ordinary least squares over the rolling window.
pub struct VolumeTrendSlope {
    period: usize,
    window: VecDeque<Decimal>,
}

impl VolumeTrendSlope {
    /// Creates a new `VolumeTrendSlope` with the given period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for VolumeTrendSlope {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let vals: Vec<f64> = self.window.iter().filter_map(|v| v.to_f64()).collect();
        if vals.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // OLS: slope = (n*Σ(x*y) - Σx*Σy) / (n*Σ(x²) - (Σx)²)
        // x = 0, 1, ..., n-1
        let sum_x: f64 = (0..self.period).map(|i| i as f64).sum();
        let sum_y: f64 = vals.iter().sum();
        let sum_xy: f64 = vals.iter().enumerate().map(|(i, y)| i as f64 * y).sum();
        let sum_x2: f64 = (0..self.period).map(|i| (i as f64) * (i as f64)).sum();
        let denom = n * sum_x2 - sum_x * sum_x;
        if denom == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let slope = (n * sum_xy - sum_x * sum_y) / denom;
        match Decimal::from_f64_retain(slope) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "VolumeTrendSlope" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(v: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(110),
            low: dec!(90),
            close: dec!(100),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vts_constant_zero_slope() {
        // Constant volume → slope = 0
        let mut sig = VolumeTrendSlope::new(3).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        let v = sig.update(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vts_increasing_positive_slope() {
        // Volume: 1000, 2000, 3000 → slope = 1000
        let mut sig = VolumeTrendSlope::new(3).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("2000")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("3000")).unwrap() {
            assert!(v > dec!(0), "expected positive slope, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
