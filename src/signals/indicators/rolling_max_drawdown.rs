//! Rolling Maximum Drawdown indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Maximum peak-to-trough drawdown of close prices within the rolling window.
///
/// `max_drawdown = max(peak - trough) / peak` for all peak-trough pairs in window.
///
/// Returns the worst percentage drawdown experienced in the last N bars.
/// High values: significant pullbacks occurred in the window.
/// Low values: price moved without major reversals (strong trend).
pub struct RollingMaxDrawdown {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl RollingMaxDrawdown {
    /// Creates a new `RollingMaxDrawdown` with the given rolling period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(period) })
    }
}

impl Signal for RollingMaxDrawdown {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Find max drawdown: for each peak, find minimum trough after it
        let vals: Vec<Decimal> = self.closes.iter().cloned().collect();
        let mut max_dd = Decimal::ZERO;
        for i in 0..vals.len() {
            if vals[i].is_zero() { continue; }
            for j in (i+1)..vals.len() {
                if vals[j] < vals[i] {
                    let dd = (vals[i] - vals[j]) / vals[i];
                    if dd > max_dd { max_dd = dd; }
                }
            }
        }
        Ok(SignalValue::Scalar(max_dd * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "RollingMaxDrawdown" }
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
    fn test_rmd_no_drawdown() {
        // Strictly rising → no trough after peak → drawdown = 0
        let mut sig = RollingMaxDrawdown::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("105")).unwrap();
        let v = sig.update(&bar("110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rmd_ten_percent_drawdown() {
        // Peak=110, trough=99 → dd = 11/110 * 100 = 10%
        let mut sig = RollingMaxDrawdown::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("110")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("99")).unwrap() {
            // (110-99)/110*100 = 10%
            assert_eq!(v, dec!(10));
        } else {
            panic!("expected Scalar");
        }
    }
}
