//! Volume Price Impact indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `|close - prev_close| / volume`.
///
/// Measures price change per unit of volume (market impact / price efficiency).
/// Low values indicate high liquidity; high values indicate thin liquidity.
/// Bars with zero volume are skipped.
pub struct VolumePriceImpact {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumePriceImpact {
    /// Creates a new `VolumePriceImpact` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for VolumePriceImpact {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !bar.volume.is_zero() {
                let impact = (bar.close - pc).abs() / bar.volume;
                self.window.push_back(impact);
                self.sum += impact;
                if self.window.len() > self.period {
                    if let Some(old) = self.window.pop_front() {
                        self.sum -= old;
                    }
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumePriceImpact" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str, v: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vpi_no_price_change() {
        let mut sig = VolumePriceImpact::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        let v = sig.update(&bar("100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vpi_basic_impact() {
        // Price change = 1, volume = 1000 → impact = 0.001
        let mut sig = VolumePriceImpact::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap(); // seeds prev_close=100
        sig.update(&bar("101", "1000")).unwrap(); // impact=1/1000=0.001
        let v = sig.update(&bar("102", "1000")).unwrap(); // impact=0.001, avg=0.001
        assert_eq!(v, SignalValue::Scalar(dec!(0.001)));
    }
}
