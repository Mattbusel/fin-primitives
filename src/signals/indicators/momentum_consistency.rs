//! Momentum Consistency indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling fraction of bars where `close[t] > close[t-1]` (up-bar ratio).
///
/// Measures directional consistency of price movement over N bars.
/// 1.0: every bar closed higher than the prior bar (persistent uptrend).
/// 0.0: every bar closed lower than the prior bar (persistent downtrend).
/// 0.5: balanced up/down movement.
pub struct MomentumConsistency {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<u8>, // 1 for up, 0 for flat/down
    up_count: usize,
}

impl MomentumConsistency {
    /// Creates a new `MomentumConsistency` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
            up_count: 0,
        })
    }
}

impl Signal for MomentumConsistency {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let up: u8 = if bar.close > pc { 1 } else { 0 };
            self.window.push_back(up);
            self.up_count += up as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.up_count -= old as usize;
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let ratio = Decimal::from(self.up_count as u32) / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
        self.up_count = 0;
    }
    fn name(&self) -> &str { "MomentumConsistency" }
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
    fn test_mc_all_up() {
        // All rising → ratio = 1
        let mut sig = MomentumConsistency::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        let v = sig.update(&bar("102")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_mc_half_up() {
        // Alternating up/down → ratio = 0.5
        let mut sig = MomentumConsistency::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap(); // up
        let v = sig.update(&bar("100")).unwrap(); // down; window=[up,down], up_count=1
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }
}
