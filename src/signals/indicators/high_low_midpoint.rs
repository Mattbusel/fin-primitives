//! High-Low Midpoint indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of the bar midpoint: `(high + low) / 2`.
///
/// A simple measure of the central price level over the period,
/// less affected by open/close noise than a simple close SMA.
pub struct HighLowMidpoint {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl HighLowMidpoint {
    /// Creates a new `HighLowMidpoint` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for HighLowMidpoint {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = (bar.high + bar.low) / Decimal::TWO;
        self.window.push_back(mid);
        self.sum += mid;
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
    fn name(&self) -> &str { "HighLowMidpoint" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_high_low_midpoint_basic() {
        let mut sig = HighLowMidpoint::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap();  // mid = 100
        let v = sig.update(&bar("120", "80")).unwrap();  // mid = 100
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_high_low_midpoint_rolling() {
        let mut sig = HighLowMidpoint::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap();  // mid=100
        sig.update(&bar("120", "100")).unwrap(); // mid=110, avg=(100+110)/2=105
        let v = sig.update(&bar("130", "110")).unwrap(); // mid=120, avg=(110+120)/2=115
        assert_eq!(v, SignalValue::Scalar(dec!(115)));
    }
}
