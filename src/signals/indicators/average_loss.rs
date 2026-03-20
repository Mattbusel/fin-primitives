//! Average Loss indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of negative close returns (losses only), returned as positive values.
///
/// Used as a component in RSI calculation and standalone bearish strength measure.
/// Positive returns contribute 0 to the average.
/// Returns 0 when no losses exist in the window.
pub struct AverageLoss {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl AverageLoss {
    /// Creates a new `AverageLoss` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for AverageLoss {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let loss = if bar.close < pc { pc - bar.close } else { Decimal::ZERO };
            self.window.push_back(loss);
            self.sum += loss;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
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
    fn name(&self) -> &str { "AverageLoss" }
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
    fn test_al_all_down() {
        // -2, -2, -2 → avg_loss = 2 (positive)
        let mut sig = AverageLoss::new(3).unwrap();
        sig.update(&bar("106")).unwrap();
        sig.update(&bar("104")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_al_no_losses() {
        // All up → avg_loss = 0
        let mut sig = AverageLoss::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        sig.update(&bar("104")).unwrap();
        let v = sig.update(&bar("106")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
