//! Open-to-High Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(high - open) / (high - low)`.
///
/// Measures how early in the bar the high tends to form:
/// Values near 1.0: high forms near end of bar (bullish, late surge).
/// Values near 0.0: high forms near start of bar (bearish, sells off from open).
/// Bars with zero range are skipped.
pub struct OpenToHighRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenToHighRatio {
    /// Creates a new `OpenToHighRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenToHighRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if !range.is_zero() {
            let ratio = (bar.high - bar.open) / range;
            self.window.push_back(ratio);
            self.sum += ratio;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "OpenToHighRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: o.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_othr_open_at_low() {
        // open=low → high-open = full range → ratio = 1
        let mut sig = OpenToHighRatio::new(2).unwrap();
        sig.update(&bar("90", "110", "90")).unwrap();
        let v = sig.update(&bar("90", "110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_othr_open_at_high() {
        // open=high → high-open = 0 → ratio = 0
        let mut sig = OpenToHighRatio::new(2).unwrap();
        sig.update(&bar("110", "110", "90")).unwrap();
        let v = sig.update(&bar("110", "110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
