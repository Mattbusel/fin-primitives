//! Mean Reversion Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Z-score of current close relative to rolling SMA and standard deviation.
///
/// `(close - SMA) / std_dev(closes)`
/// Positive: price above mean (potential sell signal in mean-reverting markets).
/// Negative: price below mean (potential buy signal in mean-reverting markets).
/// Returns Scalar(0) when std_dev is zero (flat price series).
pub struct MeanReversionScore {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl MeanReversionScore {
    /// Creates a new `MeanReversionScore` with the given period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for MeanReversionScore {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let vals: Vec<f64> = self.window.iter().filter_map(|v| v.to_f64()).collect();
        if vals.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / (n - 1.0);
        let std_dev = var.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let current = bar.close.to_f64().unwrap_or(mean);
        let z = (current - mean) / std_dev;
        match Decimal::from_f64_retain(z) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "MeanReversionScore" }
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
    fn test_mrs_flat_is_zero() {
        // Constant price → z-score = 0
        let mut sig = MeanReversionScore::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_mrs_above_mean_positive() {
        // [100, 100, 110] → mean≈103.3, current=110 → z > 0
        let mut sig = MeanReversionScore::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("110")).unwrap() {
            assert!(v > dec!(0), "expected positive z-score, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
