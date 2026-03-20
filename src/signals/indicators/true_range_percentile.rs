//! True Range Percentile indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling percentile rank of the current True Range within the period window.
///
/// Returns 0-100: how large the current TR is relative to the last N bars.
/// Useful for identifying unusually high or low volatility bars.
pub struct TrueRangePercentile {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
}

impl TrueRangePercentile {
    /// Creates a new `TrueRangePercentile` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for TrueRangePercentile {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = match self.prev_close {
            None => bar.high - bar.low,
            Some(pc) => {
                let hl = bar.high - bar.low;
                let hc = (bar.high - pc).abs();
                let lc = (bar.low - pc).abs();
                hl.max(hc).max(lc)
            }
        };
        self.prev_close = Some(bar.close);

        self.window.push_back(tr);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let current = tr;
        let below = self.window.iter().filter(|&&v| v < current).count();
        let pct = Decimal::from(below as u32) / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); }
    fn name(&self) -> &str { "TrueRangePercentile" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_tr_percentile_largest() {
        // All bars same TR except last which is largest
        let mut sig = TrueRangePercentile::new(3).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap(); // TR=20
        sig.update(&bar("110", "90", "100")).unwrap(); // TR=20
        let v = sig.update(&bar("130", "70", "100")).unwrap(); // TR=60 -> largest
        // 2 values below 60 out of 3 => 2/3 * 100 ≈ 66.6
        if let SignalValue::Scalar(x) = v {
            assert!(x > dec!(50), "largest TR should be in high percentile, got {}", x);
        }
    }

    #[test]
    fn test_tr_percentile_smallest() {
        let mut sig = TrueRangePercentile::new(3).unwrap();
        sig.update(&bar("130", "70", "100")).unwrap(); // TR=60
        sig.update(&bar("130", "70", "100")).unwrap(); // TR=60
        let v = sig.update(&bar("110", "90", "100")).unwrap(); // TR=20 -> smallest
        // 0 values below 20 => 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
