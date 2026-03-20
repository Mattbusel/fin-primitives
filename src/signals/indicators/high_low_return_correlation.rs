//! High-Low Return Correlation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Pearson correlation between high returns and low returns over rolling period.
///
/// `high_return[t] = (high[t] - high[t-1]) / high[t-1]`
/// `low_return[t] = (low[t] - low[t-1]) / low[t-1]`
///
/// High correlation (~1): highs and lows move together (trending channels).
/// Low/negative correlation: highs and lows diverge (range expansion/contraction).
pub struct HighLowReturnCorrelation {
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    high_rets: VecDeque<f64>,
    low_rets: VecDeque<f64>,
}

impl HighLowReturnCorrelation {
    /// Creates a new `HighLowReturnCorrelation` with the given period (min 3).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 3 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_high: None,
            prev_low: None,
            high_rets: VecDeque::with_capacity(period),
            low_rets: VecDeque::with_capacity(period),
        })
    }

    fn pearson(xs: &[f64], ys: &[f64]) -> f64 {
        let n = xs.len() as f64;
        if n < 2.0 { return 0.0; }
        let mx = xs.iter().sum::<f64>() / n;
        let my = ys.iter().sum::<f64>() / n;
        let num: f64 = xs.iter().zip(ys.iter()).map(|(x, y)| (x - mx) * (y - my)).sum();
        let dx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum::<f64>().sqrt();
        let dy: f64 = ys.iter().map(|y| (y - my).powi(2)).sum::<f64>().sqrt();
        if dx == 0.0 || dy == 0.0 { return 0.0; }
        num / (dx * dy)
    }
}

impl Signal for HighLowReturnCorrelation {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            if !ph.is_zero() && !pl.is_zero() {
                if let (Some(hr), Some(lr)) = (
                    ((bar.high - ph) / ph).to_f64(),
                    ((bar.low - pl) / pl).to_f64(),
                ) {
                    self.high_rets.push_back(hr);
                    self.low_rets.push_back(lr);
                    if self.high_rets.len() > self.period {
                        self.high_rets.pop_front();
                        self.low_rets.pop_front();
                    }
                }
            }
        }
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);

        if self.high_rets.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let xs: Vec<f64> = self.high_rets.iter().cloned().collect();
        let ys: Vec<f64> = self.low_rets.iter().cloned().collect();
        let corr = Self::pearson(&xs, &ys);
        match Decimal::from_f64_retain(corr) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.high_rets.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
        self.high_rets.clear();
        self.low_rets.clear();
    }
    fn name(&self) -> &str { "HighLowReturnCorrelation" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_hlrc_not_ready() {
        let mut sig = HighLowReturnCorrelation::new(3).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        sig.update(&bar("115", "85")).unwrap();
        let v = sig.update(&bar("120", "80")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_hlrc_perfect_correlation() {
        // High and low move proportionally together → correlation near 1
        let mut sig = HighLowReturnCorrelation::new(3).unwrap();
        sig.update(&bar("100", "90")).unwrap();
        sig.update(&bar("110", "99")).unwrap();  // +10%, +10%
        sig.update(&bar("121", "108.9")).unwrap(); // +10%, +10%
        sig.update(&bar("133.1", "119.79")).unwrap(); // +10%, +10%
        if let SignalValue::Scalar(v) = sig.update(&bar("146.41", "131.769")).unwrap() {
            // All returns identical → correlation = 1 (or very close)
            assert!(v > dec!(0.99), "expected near-perfect correlation, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
