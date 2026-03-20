//! Close-Below-Prior-Low indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling percentage of bars where `close < prior bar low`.
///
/// Measures breakdown frequency — how often price closes below the previous bar's low.
/// High values indicate persistent downside breakdowns.
pub struct CloseBelowLowPrev {
    period: usize,
    prev_low: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl CloseBelowLowPrev {
    /// Creates a new `CloseBelowLowPrev` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_low: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for CloseBelowLowPrev {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pl) = self.prev_low {
            let below: u8 = if bar.close < pl { 1 } else { 0 };
            self.window.push_back(below);
            self.count += below as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_low = Some(bar.low);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let pct = Decimal::from(self.count as u32) / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_low = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "CloseBelowLowPrev" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(l: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(200),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_close_below_low_prev_always_below() {
        let mut sig = CloseBelowLowPrev::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap(); // seeds prev_low=100
        sig.update(&bar("90", "95")).unwrap(); // 95 < 100 ✓, seeds prev_low=90
        let v = sig.update(&bar("80", "85")).unwrap(); // 85 < 90 ✓ → 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_close_below_low_prev_never_below() {
        let mut sig = CloseBelowLowPrev::new(2).unwrap();
        sig.update(&bar("80", "90")).unwrap(); // seeds prev_low=80
        sig.update(&bar("85", "95")).unwrap(); // 95 > 80 ✗
        let v = sig.update(&bar("90", "100")).unwrap(); // 100 > 85 ✗ → 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
