//! High-Low Divergence indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(high - prev_high) - (prev_low - low)`.
///
/// Positive values: highs expanding faster than lows are contracting (bullish expansion).
/// Negative values: lows dropping faster than highs are rising (bearish expansion).
/// Near zero: symmetric range expansion or contraction.
pub struct HighLowDivergence {
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl HighLowDivergence {
    /// Creates a new `HighLowDivergence` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_high: None,
            prev_low: None,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for HighLowDivergence {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            let div = (bar.high - ph) - (pl - bar.low);
            self.window.push_back(div);
            self.sum += div;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_high = None; self.prev_low = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "HighLowDivergence" }
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
    fn test_hl_divergence_symmetric_expansion() {
        // Both high and low expand equally → divergence = 0
        let mut sig = HighLowDivergence::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap(); // seeds
        sig.update(&bar("115", "85")).unwrap(); // +5, -5 → div=5-5=0
        let v = sig.update(&bar("120", "80")).unwrap(); // +5, -5 → div=5-5=0, avg=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hl_divergence_bullish() {
        // High expanding, low stable → positive divergence
        let mut sig = HighLowDivergence::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap(); // seeds
        sig.update(&bar("115", "90")).unwrap(); // high+5, low+0 → div=5-0=5
        let v = sig.update(&bar("120", "90")).unwrap(); // div=5, avg=5
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }
}
