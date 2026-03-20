//! Body Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling sum of signed body sizes: `close - open` per bar.
///
/// Positive sums indicate net bullish body movement.
/// Negative sums indicate net bearish body movement.
/// Measures cumulative conviction of price direction over the window.
pub struct BodyMomentum {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl BodyMomentum {
    /// Creates a new `BodyMomentum` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for BodyMomentum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.close - bar.open;
        self.window.push_back(body);
        self.sum += body;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "BodyMomentum" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: c.parse().unwrap(),
            low: o.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_bm_all_bullish() {
        // Each bar: open=100, close=105 → body=+5, sum over 3 bars = 15
        let mut sig = BodyMomentum::new(3).unwrap();
        sig.update(&bar("100", "105")).unwrap();
        sig.update(&bar("100", "105")).unwrap();
        let v = sig.update(&bar("100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(15)));
    }

    #[test]
    fn test_bm_mixed_zero() {
        // Alternating +5 and -5 → sum = 0 over period of 2
        let mut sig = BodyMomentum::new(2).unwrap();
        sig.update(&bar("100", "105")).unwrap(); // +5
        let v = sig.update(&bar("105", "100")).unwrap(); // -5, sum=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
