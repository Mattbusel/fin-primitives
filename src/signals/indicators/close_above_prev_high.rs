//! Close Above Previous High indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where `close > previous bar's high`.
///
/// Signals breakout strength — how often price closes above the prior bar's high.
/// High values indicate persistent bullish breakouts over the period.
pub struct CloseAbovePrevHigh {
    period: usize,
    prev_high: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl CloseAbovePrevHigh {
    /// Creates a new `CloseAbovePrevHigh` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_high: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for CloseAbovePrevHigh {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(ph) = self.prev_high {
            let hit: u8 = if bar.close > ph { 1 } else { 0 };
            self.window.push_back(hit);
            self.count += hit as usize;
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
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_high = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "CloseAbovePrevHigh" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: h.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_caph_all_breakouts() {
        // Each close beats previous high
        let mut sig = CloseAbovePrevHigh::new(3).unwrap();
        sig.update(&bar("100", "100")).unwrap(); // seeds prev_high=100
        sig.update(&bar("105", "105")).unwrap(); // close(105) > prev_high(100) ✓
        sig.update(&bar("110", "110")).unwrap(); // close(110) > prev_high(105) ✓
        let v = sig.update(&bar("115", "115")).unwrap(); // close(115) > prev_high(110) ✓ → count=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_caph_no_breakouts() {
        // Close never beats previous high
        let mut sig = CloseAbovePrevHigh::new(3).unwrap();
        sig.update(&bar("110", "100")).unwrap(); // seeds prev_high=110
        sig.update(&bar("110", "100")).unwrap(); // close(100) <= prev_high(110)
        sig.update(&bar("110", "100")).unwrap(); // close(100) <= prev_high(110)
        let v = sig.update(&bar("110", "100")).unwrap(); // close(100) <= prev_high(110) → count=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
