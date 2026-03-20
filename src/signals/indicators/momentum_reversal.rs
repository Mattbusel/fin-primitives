//! Momentum Reversal indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where the return sign flips from the prior bar.
///
/// Measures how often the market reverses direction. High values indicate
/// a choppy, mean-reverting market; low values indicate trending behaviour.
pub struct MomentumReversal {
    period: usize,
    prev_close: Option<Decimal>,
    prev_sign: i8,
    window: VecDeque<u8>,
    count: usize,
}

impl MomentumReversal {
    /// Creates a new `MomentumReversal` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, prev_sign: 0, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for MomentumReversal {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let sign: i8 = if bar.close > pc { 1 } else if bar.close < pc { -1 } else { 0 };
            // Reversal = sign changed AND both are non-zero
            let reversed: u8 = if sign != 0 && self.prev_sign != 0 && sign != self.prev_sign { 1 } else { 0 };
            self.window.push_back(reversed);
            self.count += reversed as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
            self.prev_sign = sign;
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.prev_sign = 0; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "MomentumReversal" }
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
    fn test_momentum_reversal_alternating() {
        let mut sig = MomentumReversal::new(3).unwrap();
        sig.update(&bar("100")).unwrap(); // seeds
        sig.update(&bar("102")).unwrap(); // +1, prev_sign=+1
        sig.update(&bar("100")).unwrap(); // -1, reversal=1
        sig.update(&bar("102")).unwrap(); // +1, reversal=1
        let v = sig.update(&bar("100")).unwrap(); // -1, reversal=1, window=[1,1,1]=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_momentum_reversal_trending() {
        let mut sig = MomentumReversal::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap(); // +1
        sig.update(&bar("102")).unwrap(); // +1, no reversal
        sig.update(&bar("103")).unwrap(); // +1, no reversal
        let v = sig.update(&bar("104")).unwrap(); // +1, no reversal, count=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
