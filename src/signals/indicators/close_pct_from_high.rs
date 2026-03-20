//! Close Percentage From N-Bar High indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Percentage distance of current close from the N-bar rolling high.
///
/// `(rolling_high - close) / rolling_high * 100`
///
/// Values near 0: close near recent high (strong momentum).
/// Higher values: close well below recent high (pullback or weakness).
/// Always non-negative.
pub struct ClosePctFromHigh {
    period: usize,
    window: VecDeque<Decimal>,
}

impl ClosePctFromHigh {
    /// Creates a new `ClosePctFromHigh` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for ClosePctFromHigh {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.high);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let rolling_high = self.window.iter().cloned().fold(Decimal::MIN, Decimal::max);
        if rolling_high.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let pct = (rolling_high - bar.close) / rolling_high * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "ClosePctFromHigh" }
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
    fn test_cpfh_at_high() {
        // close = rolling_high → 0%
        let mut sig = ClosePctFromHigh::new(3).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        sig.update(&bar("105", "105")).unwrap();
        let v = sig.update(&bar("110", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cpfh_below_high() {
        // rolling_high=110, close=99 → (110-99)/110*100 = 10%
        let mut sig = ClosePctFromHigh::new(2).unwrap();
        sig.update(&bar("110", "110")).unwrap();
        let v = sig.update(&bar("100", "99")).unwrap();
        // rolling_high=max(110,100)=110, close=99 → (110-99)/110*100 = 10
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
