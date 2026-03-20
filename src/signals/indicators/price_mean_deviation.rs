//! Price Mean Deviation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling mean absolute deviation (MAD) of close prices from their rolling mean.
///
/// `mean(|close - SMA(close)|)` over the rolling period.
///
/// A robust volatility measure, less sensitive to outliers than standard deviation.
/// Useful as a substitute for std dev in noisy or fat-tailed markets.
pub struct PriceMeanDeviation {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceMeanDeviation {
    /// Creates a new `PriceMeanDeviation` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for PriceMeanDeviation {
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

        let mean = self.sum / Decimal::from(self.period as u32);
        let mad: Decimal = self.window.iter()
            .map(|&c| (c - mean).abs())
            .sum::<Decimal>() / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(mad))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "PriceMeanDeviation" }
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
    fn test_pmd_flat_zero() {
        // Constant prices → MAD = 0
        let mut sig = PriceMeanDeviation::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pmd_symmetric() {
        // [90, 100, 110]: mean=100, deviations=[10, 0, 10] → MAD=20/3 ≈ 6.666...
        let mut sig = PriceMeanDeviation::new(3).unwrap();
        sig.update(&bar("90")).unwrap();
        sig.update(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("110")).unwrap() {
            assert!(v > dec!(0), "expected non-zero MAD, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
