//! Body-to-Range Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `|close - open| / (high - low)`.
///
/// Measures how much of the total bar range is covered by the candle body.
/// 1.0 = marubozu (no wicks), near 0 = spinning top / doji.
pub struct BodyToRangeRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl BodyToRangeRatio {
    /// Creates a new `BodyToRangeRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for BodyToRangeRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let ratio = if range.is_zero() {
            Decimal::ZERO
        } else {
            bar.body_size() / range
        };
        self.window.push_back(ratio);
        self.sum += ratio;
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
    fn name(&self) -> &str { "BodyToRangeRatio" }
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
    fn test_body_to_range_marubozu() {
        // Full body: open=low, close=high => ratio=1
        let mut sig = BodyToRangeRatio::new(2).unwrap();
        sig.update(&bar("100", "110", "100", "110")).unwrap();
        let v = sig.update(&bar("100", "110", "100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_body_to_range_doji() {
        // Doji: open == close => body=0, ratio=0
        let mut sig = BodyToRangeRatio::new(2).unwrap();
        sig.update(&bar("100", "110", "90", "100")).unwrap();
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
