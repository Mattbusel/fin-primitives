//! Return Auto-Correlation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Lag-1 autocorrelation of close returns over the rolling window.
///
/// Measures whether returns tend to continue (positive autocorrelation = momentum)
/// or reverse (negative autocorrelation = mean-reversion).
/// Returns a value in [-1, 1].
pub struct ReturnAutoCorrelation {
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
}

impl ReturnAutoCorrelation {
    /// Creates a new `ReturnAutoCorrelation` with the given rolling period (min 3).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 3 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, returns: VecDeque::with_capacity(period) })
    }
}

impl Signal for ReturnAutoCorrelation {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc) / pc;
                self.returns.push_back(ret);
                if self.returns.len() > self.period {
                    self.returns.pop_front();
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.returns.len();
        let vals: Vec<f64> = self.returns.iter()
            .filter_map(|r| r.to_f64())
            .collect();
        if vals.len() < n {
            return Ok(SignalValue::Unavailable);
        }

        // Lag-1 autocorrelation: corr(r[t], r[t-1])
        let n_pairs = n - 1;
        if n_pairs < 2 {
            return Ok(SignalValue::Unavailable);
        }

        let x: Vec<f64> = vals[..n_pairs].to_vec();  // r[t]
        let y: Vec<f64> = vals[1..].to_vec();         // r[t+1]

        let np = n_pairs as f64;
        let mx = x.iter().sum::<f64>() / np;
        let my = y.iter().sum::<f64>() / np;

        let num: f64 = x.iter().zip(y.iter()).map(|(xi, yi)| (xi - mx) * (yi - my)).sum();
        let dx: f64 = x.iter().map(|xi| (xi - mx) * (xi - mx)).sum::<f64>().sqrt();
        let dy: f64 = y.iter().map(|yi| (yi - my) * (yi - my)).sum::<f64>().sqrt();

        let denom = dx * dy;
        if denom == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let corr = num / denom;
        match Decimal::from_f64_retain(corr) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.returns.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.returns.clear(); }
    fn name(&self) -> &str { "ReturnAutoCorrelation" }
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
    fn test_autocorr_not_ready() {
        let mut sig = ReturnAutoCorrelation::new(4).unwrap();
        for _ in 0..4 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_autocorr_trending_positive() {
        // Steady uptrend: each return similar → positive autocorrelation
        let mut sig = ReturnAutoCorrelation::new(5).unwrap();
        let prices = ["100", "102", "104", "106", "108", "110"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = sig.update(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(x) = last {
            assert!(x > dec!(0), "trending series should have positive autocorr, got {}", x);
        }
    }
}
