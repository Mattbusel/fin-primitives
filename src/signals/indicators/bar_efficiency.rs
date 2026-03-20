//! Bar Efficiency indicator -- rolling body-to-range ratio.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of |close - open| / (high - low) * 100.
/// Measures candle directionality: 0 = pure doji, 100 = full-body candle.
pub struct BarEfficiency {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl BarEfficiency {
    /// Creates a new `BarEfficiency` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(0));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for BarEfficiency {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let body = if bar.close >= bar.open {
            bar.close - bar.open
        } else {
            bar.open - bar.close
        };
        let eff = if range.is_zero() {
            Decimal::ZERO
        } else {
            body / range * Decimal::ONE_HUNDRED
        };
        self.window.push_back(eff);
        self.sum += eff;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
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
    fn name(&self) -> &str { "BarEfficiency" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_bar_efficiency_full_body() {
        let mut sig = BarEfficiency::new(3).unwrap();
        // Full-body up candles: body = range, efficiency = 100
        assert_eq!(sig.update(&bar("100", "110", "100", "110")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("100", "110", "100", "110")).unwrap(), SignalValue::Unavailable);
        let v = sig.update(&bar("100", "110", "100", "110")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert_eq!(x, dec!(100));
        }
    }

    #[test]
    fn test_bar_efficiency_doji() {
        let mut sig = BarEfficiency::new(2).unwrap();
        // Doji: open == close
        sig.update(&bar("100", "110", "90", "100")).unwrap();
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert_eq!(x, dec!(0));
        }
    }
}
