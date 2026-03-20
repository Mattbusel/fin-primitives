//! Price Oscillator Sign indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Sign of the price oscillator: +1 if short SMA > long SMA, -1 if below, 0 if equal.
///
/// Captures the direction (not magnitude) of trend: fast average vs. slow average.
/// +1: short-term momentum is above long-term trend (bullish regime).
/// -1: short-term momentum is below long-term trend (bearish regime).
///  0: SMAs are equal (crossover point or perfectly flat).
///
/// Requires `short_period < long_period`.
pub struct PriceOscillatorSign {
    short_period: usize,
    long_period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceOscillatorSign {
    /// Creates a new `PriceOscillatorSign`.
    ///
    /// `short_period`: faster SMA period.
    /// `long_period`: slower SMA period.
    pub fn new(short_period: usize, long_period: usize) -> Result<Self, FinError> {
        if short_period == 0 || long_period == 0 {
            return Err(FinError::InvalidPeriod(if short_period == 0 { short_period } else { long_period }));
        }
        if short_period >= long_period {
            return Err(FinError::InvalidPeriod(short_period));
        }
        Ok(Self {
            short_period,
            long_period,
            window: VecDeque::with_capacity(long_period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceOscillatorSign {
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

        let sign: i32 = if short_sma > long_sma { 1 } else if short_sma < long_sma { -1 } else { 0 };
        Ok(SignalValue::Scalar(Decimal::from(sign)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.long_period }
    fn period(&self) -> usize { self.long_period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "PriceOscillatorSign" }
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
    fn test_pos_uptrend_bullish() {
        // Rising prices → short SMA > long SMA → +1
        let mut sig = PriceOscillatorSign::new(2, 4).unwrap();
        for c in &["100", "101", "102", "103"] {
            sig.update(&bar(c)).unwrap();
        }
        // short SMA(2): avg(102,103)=102.5, long SMA(4): avg(100,101,102,103)=101.5 → +1
        let v = sig.update(&bar("104")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_pos_flat_zero() {
        // Constant prices → short SMA = long SMA → 0
        let mut sig = PriceOscillatorSign::new(2, 4).unwrap();
        for c in &["100", "100", "100", "100"] {
            sig.update(&bar(c)).unwrap();
        }
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
