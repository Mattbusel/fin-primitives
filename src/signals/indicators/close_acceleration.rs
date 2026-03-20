//! Close Acceleration indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rate of change of N-bar momentum: `momentum[t] - momentum[t-1]`.
///
/// Where momentum = `close[t] - close[t-N]`.
/// Positive values indicate accelerating upward momentum.
/// Negative values indicate decelerating upward or accelerating downward momentum.
/// Requires `2*period + 1` bars to first produce a value.
pub struct CloseAcceleration {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl CloseAcceleration {
    /// Creates a new `CloseAcceleration` with the given N-bar momentum period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(2 * period + 1) })
    }
}

impl Signal for CloseAcceleration {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > 2 * self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < 2 * self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        let n = self.period;
        let len = self.closes.len();
        // momentum[t] = close[last] - close[last-n]
        // momentum[t-1] = close[last-1] - close[last-1-n]
        let mom_t = self.closes[len - 1] - self.closes[len - 1 - n];
        let mom_t1 = self.closes[len - 2] - self.closes[len - 2 - n];
        Ok(SignalValue::Scalar(mom_t - mom_t1))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= 2 * self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "CloseAcceleration" }
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
    fn test_close_acceleration_constant_momentum() {
        // Constant +1 per bar: momentum always = period, acceleration = 0
        let mut sig = CloseAcceleration::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        sig.update(&bar("103")).unwrap();
        let v = sig.update(&bar("104")).unwrap(); // mom_t=2, mom_t1=2, accel=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_close_acceleration_not_ready() {
        let mut sig = CloseAcceleration::new(3).unwrap();
        for _ in 0..6 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        // needs 2*3+1=7 bars
    }
}
