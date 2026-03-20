//! Close Percentage From N-Bar Low indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Percentage distance of current close above the N-bar rolling low.
///
/// `(close - rolling_low) / rolling_low * 100`
///
/// Values near 0: close near recent low (weak momentum / potential reversal).
/// Higher values: close well above recent low (strong recovery or uptrend).
/// Always non-negative.
pub struct ClosePctFromLow {
    period: usize,
    window: VecDeque<Decimal>,
}

impl ClosePctFromLow {
    /// Creates a new `ClosePctFromLow` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for ClosePctFromLow {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.low);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let rolling_low = self.window.iter().cloned().fold(Decimal::MAX, Decimal::min);
        if rolling_low.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let pct = (bar.close - rolling_low) / rolling_low * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "ClosePctFromLow" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(l: &str, c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_cpfl_at_low() {
        // close = rolling_low → 0%
        let mut sig = ClosePctFromLow::new(2).unwrap();
        sig.update(&bar("90", "90")).unwrap();
        let v = sig.update(&bar("85", "85")).unwrap();
        // rolling_low=min(90,85)=85, close=85 → 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cpfl_above_low() {
        // rolling_low=90, close=99 → (99-90)/90*100 = 10%
        let mut sig = ClosePctFromLow::new(2).unwrap();
        sig.update(&bar("90", "95")).unwrap();
        let v = sig.update(&bar("92", "99")).unwrap();
        // rolling_low=min(90,92)=90, close=99 → (99-90)/90*100 = 10
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
