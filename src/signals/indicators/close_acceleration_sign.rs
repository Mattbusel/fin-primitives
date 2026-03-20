//! Close Acceleration Sign indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Sign of price acceleration: +1 if momentum increasing, -1 if decreasing, 0 if flat.
///
/// Requires 3 bars. Let:
/// - `d1 = close[t] - close[t-1]` (current velocity)
/// - `d2 = close[t-1] - close[t-2]` (previous velocity)
/// - Returns sign of `d1 - d2` (acceleration)
///
/// +1: price accelerating upward (or decelerating downward).
/// -1: price decelerating upward (or accelerating downward).
///  0: constant velocity (linear movement).
pub struct CloseAccelerationSign {
    c0: Option<Decimal>,
    c1: Option<Decimal>,
}

impl CloseAccelerationSign {
    /// Creates a new `CloseAccelerationSign` indicator.
    pub fn new() -> Self {
        Self { c0: None, c1: None }
    }
}

impl Default for CloseAccelerationSign {
    fn default() -> Self { Self::new() }
}

impl Signal for CloseAccelerationSign {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let (Some(c0), Some(c1)) = (self.c0, self.c1) {
            let d1 = bar.close - c1;
            let d2 = c1 - c0;
            let accel = d1 - d2;
            let sign: i32 = if accel > Decimal::ZERO { 1 } else if accel < Decimal::ZERO { -1 } else { 0 };
            SignalValue::Scalar(Decimal::from(sign))
        } else {
            SignalValue::Unavailable
        };
        self.c0 = self.c1;
        self.c1 = Some(bar.close);
        Ok(result)
    }

    fn is_ready(&self) -> bool { self.c0.is_some() && self.c1.is_some() }
    fn period(&self) -> usize { 3 }
    fn reset(&mut self) { self.c0 = None; self.c1 = None; }
    fn name(&self) -> &str { "CloseAccelerationSign" }
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
    fn test_cas_accelerating() {
        // Moves: +1, +2 → acceleration = +1 → sign = +1
        let mut sig = CloseAccelerationSign::new();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        let v = sig.update(&bar("103")).unwrap(); // d1=2, d2=1 → accel=+1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cas_decelerating() {
        // Moves: +3, +1 → acceleration = -2 → sign = -1
        let mut sig = CloseAccelerationSign::new();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("103")).unwrap();
        let v = sig.update(&bar("104")).unwrap(); // d1=1, d2=3 → accel=-2
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_cas_constant_velocity_zero() {
        // +2, +2 → accel=0
        let mut sig = CloseAccelerationSign::new();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("104")).unwrap(); // d1=2, d2=2 → accel=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
