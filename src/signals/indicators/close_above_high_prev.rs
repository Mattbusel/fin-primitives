//! Close-Above-Prior-High indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling percentage of bars where `close > prior bar high`.
///
/// Measures breakout frequency — how often price closes above the previous bar's high.
/// High values indicate persistent upside breakouts.
pub struct CloseAboveHighPrev {
    period: usize,
    prev_high: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl CloseAboveHighPrev {
    /// Creates a new `CloseAboveHighPrev` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_high: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for CloseAboveHighPrev {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(ph) = self.prev_high {
            let above: u8 = if bar.close > ph { 1 } else { 0 };
            self.window.push_back(above);
            self.count += above as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_high = Some(bar.high);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let pct = Decimal::from(self.count as u32) / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_high = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "CloseAboveHighPrev" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: dec!(90),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_close_above_high_prev_always_above() {
        let mut sig = CloseAboveHighPrev::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap(); // seeds prev_high=100
        sig.update(&bar("110", "105")).unwrap(); // 105 > 100 ✓, seeds prev_high=110
        let v = sig.update(&bar("120", "115")).unwrap(); // 115 > 110 ✓ → 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_close_above_high_prev_never_above() {
        let mut sig = CloseAboveHighPrev::new(2).unwrap();
        sig.update(&bar("110", "100")).unwrap(); // seeds prev_high=110
        sig.update(&bar("115", "105")).unwrap(); // 105 < 110 ✗
        let v = sig.update(&bar("120", "110")).unwrap(); // 110 < 115 ✗ → 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
