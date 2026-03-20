//! Chaikin Volatility indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Chaikin Volatility: EMA of (high - low) with rate-of-change.
/// Returns (current_ema - ema_N_bars_ago) / ema_N_bars_ago * 100.
pub struct ChaikinVolatility {
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
    history: VecDeque<Decimal>,
    bars_seen: usize,
}

impl ChaikinVolatility {
    /// Creates a new `ChaikinVolatility` with the given EMA period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let k = Decimal::TWO / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { period, ema: None, k, history: VecDeque::with_capacity(period + 1), bars_seen: 0 })
    }
}

impl Signal for ChaikinVolatility {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let hl = bar.high - bar.low;
        let ema = match self.ema {
            None => hl,
            Some(prev) => hl * self.k + prev * (Decimal::ONE - self.k),
        };
        self.ema = Some(ema);
        self.history.push_back(ema);
        self.bars_seen += 1;
        // Keep period+1 values so we can compare current vs N bars ago
        if self.history.len() > self.period + 1 {
            self.history.pop_front();
        }
        if self.bars_seen <= self.period {
            return Ok(SignalValue::Unavailable);
        }
        let past = *self.history.front().unwrap();
        if past.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar((ema - past) / past * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.bars_seen > self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.ema = None;
        self.history.clear();
        self.bars_seen = 0;
    }
    fn name(&self) -> &str { "ChaikinVolatility" }
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
    fn test_chaikin_volatility_not_ready_initially() {
        let mut sig = ChaikinVolatility::new(3).unwrap();
        for _ in 0..3 {
            assert_eq!(sig.update(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!sig.is_ready());
    }

    #[test]
    fn test_chaikin_volatility_ready_after_period_plus_one() {
        let mut sig = ChaikinVolatility::new(3).unwrap();
        for i in 0..4 {
            let v = sig.update(&bar("110", "90")).unwrap();
            if i < 3 {
                assert_eq!(v, SignalValue::Unavailable);
            } else {
                assert!(matches!(v, SignalValue::Scalar(_)));
            }
        }
    }
}
