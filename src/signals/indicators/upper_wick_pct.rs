//! Upper Wick Percentage indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of upper wick as a percentage of total bar range.
///
/// `upper_wick = high - max(open, close)`
/// `ratio = upper_wick / (high - low) * 100`
///
/// High values indicate consistent selling pressure at highs (bearish rejection).
/// Zero for doji bars (range = 0) or bars with no upper wick.
pub struct UpperWickPct {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl UpperWickPct {
    /// Creates a new `UpperWickPct` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for UpperWickPct {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let pct = if range.is_zero() {
            Decimal::ZERO
        } else {
            let body_top = bar.open.max(bar.close);
            let upper_wick = bar.high - body_top;
            upper_wick / range * Decimal::ONE_HUNDRED
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
    fn name(&self) -> &str { "UpperWickPct" }
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
    fn test_uwp_no_upper_wick() {
        // close = high → upper_wick = 0 → pct = 0
        let mut sig = UpperWickPct::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "110")).unwrap();
        let v = sig.update(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_uwp_half_upper_wick() {
        // open=90, close=90, high=110, low=90 → range=20, body_top=90, upper_wick=20 → 100%
        let mut sig = UpperWickPct::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "90")).unwrap(); // upper_wick=20, range=20 → 100%
        let v = sig.update(&bar("90", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }
}
