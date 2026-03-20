//! Typical Price Deviation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Rolling standard deviation of typical price `(high + low + close) / 3`.
///
/// Measures volatility using the typical price rather than just the close,
/// giving equal weight to the full bar's trading range.
pub struct TypicalPriceDeviation {
    period: usize,
    window: VecDeque<Decimal>,
}

impl TypicalPriceDeviation {
    /// Creates a new `TypicalPriceDeviation` with the given period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for TypicalPriceDeviation {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tp = (bar.high + bar.low + bar.close) / Decimal::from(3u32);
        self.window.push_back(tp);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let vals: Vec<f64> = self.window.iter()
            .filter_map(|v| v.to_f64())
            .collect();
        if vals.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / (n - 1.0);
        let std_dev = var.sqrt();

        match Decimal::from_f64_retain(std_dev) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "TypicalPriceDeviation" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_typical_price_deviation_not_ready() {
        let mut sig = TypicalPriceDeviation::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_typical_price_deviation_constant_zero() {
        // Identical bars → TP constant → std_dev = 0
        let mut sig = TypicalPriceDeviation::new(3).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap(); // TP=100
        sig.update(&bar("110", "90", "100")).unwrap(); // TP=100
        let v = sig.update(&bar("110", "90", "100")).unwrap(); // TP=100
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
