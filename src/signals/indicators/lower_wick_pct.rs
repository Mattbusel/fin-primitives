//! Lower Wick Percentage indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of lower wick as a percentage of total bar range.
///
/// `lower_wick = min(open, close) - low`
/// `ratio = lower_wick / (high - low) * 100`
///
/// High values indicate consistent buying support at lows (bullish rejection).
/// Zero for doji bars (range = 0) or bars with no lower wick.
pub struct LowerWickPct {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl LowerWickPct {
    /// Creates a new `LowerWickPct` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for LowerWickPct {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let pct = if range.is_zero() {
            Decimal::ZERO
        } else {
            let lower_wick = bar.lower_wick();
            lower_wick / range * Decimal::ONE_HUNDRED
        };
        self.window.push_back(pct);
        self.sum += pct;
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
    fn name(&self) -> &str { "LowerWickPct" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_lwp_no_lower_wick() {
        // open = close = low → lower_wick = 0 → pct = 0
        let mut sig = LowerWickPct::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "90")).unwrap();
        let v = sig.update(&bar("90", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_lwp_full_lower_wick() {
        // open=110, close=110, high=110, low=90 → body_bottom=110, lower_wick=20, range=20 → 100%
        let mut sig = LowerWickPct::new(2).unwrap();
        sig.update(&bar("110", "110", "90", "110")).unwrap(); // lower_wick=20/20 = 100%
        let v = sig.update(&bar("110", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }
}
