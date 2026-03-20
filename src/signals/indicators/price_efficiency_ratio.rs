//! Price Efficiency Ratio indicator (Kaufman's ER).

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Kaufman's Efficiency Ratio: `|close[t] - close[t-N]| / sum(|close[i] - close[i-1]|)`.
///
/// Measures how efficiently price moves. 1 = straight-line trend, ~0 = choppy/noisy market.
pub struct PriceEfficiencyRatio {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PriceEfficiencyRatio {
    /// Creates a new `PriceEfficiencyRatio` with the given period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(period + 1) })
    }
}

impl Signal for PriceEfficiencyRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        let net = (self.closes.back().unwrap() - self.closes.front().unwrap()).abs();
        let path: Decimal = self.closes.iter()
            .zip(self.closes.iter().skip(1))
            .map(|(a, b)| (*b - *a).abs())
            .sum();
        if path.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar(net / path))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "PriceEfficiencyRatio" }
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
    fn test_efficiency_ratio_straight_line() {
        // Monotonic trend: path == net => ER = 1
        let mut sig = PriceEfficiencyRatio::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_efficiency_ratio_choppy() {
        // Up-down alternating: net < path => ER < 1
        let mut sig = PriceEfficiencyRatio::new(4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        // net = 0, path = 8
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
