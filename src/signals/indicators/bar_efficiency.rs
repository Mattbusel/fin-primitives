//! Bar Efficiency indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of bar efficiency: `|close - open| / (high - low)`.
///
/// Measures how directionally efficient each bar is relative to its range.
/// 1.0: perfectly efficient — close at high (bull) or low (bear) relative to open.
/// 0.0: no net movement — doji-like bars with wide range.
/// Returns Unavailable for bars where high == low.
pub struct BarEfficiency {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl BarEfficiency {
    /// Creates a new `BarEfficiency` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for BarEfficiency {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let body = (bar.close - bar.open).abs();
        let eff = body / range;

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
        Ok(SignalValue::Scalar(self.sum / Decimal::from(self.period as u32)))
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
    fn test_be_full_efficiency() {
        // close at high, open at low → body=range → efficiency=1
        let mut sig = BarEfficiency::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "110")).unwrap(); // eff=20/20=1
        let v = sig.update(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_be_doji_zero() {
        // open=close → body=0 → efficiency=0
        let mut sig = BarEfficiency::new(2).unwrap();
        sig.update(&bar("100", "110", "90", "100")).unwrap(); // eff=0
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
