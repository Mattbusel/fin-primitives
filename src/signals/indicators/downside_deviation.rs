//! Downside Deviation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Rolling standard deviation of only negative close returns (downside risk).
///
/// Used as a component of the Sortino ratio. Ignores positive returns,
/// focusing purely on the volatility of losses.
/// Returns 0 when there are no negative returns in the window.
pub struct DownsideDeviation {
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
}

impl DownsideDeviation {
    /// Creates a new `DownsideDeviation` with the given period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, returns: VecDeque::with_capacity(period) })
    }
}

impl Signal for DownsideDeviation {
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

        let neg_vals: Vec<f64> = self.returns.iter()
            .filter_map(|r| {
                let fv = r.to_f64()?;
                if fv < 0.0 { Some(fv) } else { None }
            })
            .collect();

        if neg_vals.len() < 2 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let n = neg_vals.len() as f64;
        let mean = neg_vals.iter().sum::<f64>() / n;
        let var = neg_vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / (n - 1.0);

        match Decimal::from_f64_retain(var.sqrt()) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.returns.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.returns.clear(); }
    fn name(&self) -> &str { "DownsideDeviation" }
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
    fn test_downside_deviation_no_losses() {
        // Only up returns → no downside → returns 0
        let mut sig = DownsideDeviation::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_downside_deviation_not_ready() {
        let mut sig = DownsideDeviation::new(4).unwrap();
        for _ in 0..4 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }
}
