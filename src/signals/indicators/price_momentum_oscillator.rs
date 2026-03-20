//! Price Momentum Oscillator indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Short-period minus long-period rolling close SMA.
///
/// Positive: short-term trend above long-term trend (bullish momentum).
/// Negative: short-term trend below long-term trend (bearish momentum).
/// `short_period` must be less than `long_period`.
pub struct PriceMomentumOscillator {
    short_period: usize,
    long_period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceMomentumOscillator {
    /// Creates a new `PriceMomentumOscillator` with short and long SMA periods.
    pub fn new(short_period: usize, long_period: usize) -> Result<Self, FinError> {
        if short_period == 0 || long_period <= short_period {
            return Err(FinError::InvalidPeriod(long_period));
        }
        Ok(Self {
            short_period,
            long_period,
            window: VecDeque::with_capacity(long_period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceMomentumOscillator {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.long_period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        let long_sma = self.sum / Decimal::from(self.long_period as u32);
        let short_sum: Decimal = self.window.iter().rev().take(self.short_period).sum();
        let short_sma = short_sum / Decimal::from(self.short_period as u32);
        Ok(SignalValue::Scalar(short_sma - long_sma))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.long_period }
    fn period(&self) -> usize { self.long_period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "PriceMomentumOscillator" }
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
    fn test_pmo_equal_smas_zero() {
        // Constant price → both SMAs equal → oscillator = 0
        let mut sig = PriceMomentumOscillator::new(2, 4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pmo_trending_up_positive() {
        // Rising prices: short SMA > long SMA → positive
        let mut sig = PriceMomentumOscillator::new(2, 4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap();
        // long_sma = (100+101+102+103)/4 = 101.5, short_sma = (102+103)/2 = 102.5
        // oscillator = 102.5 - 101.5 = 1.0
        assert_eq!(v, SignalValue::Scalar(dec!(1.0)));
    }
}
