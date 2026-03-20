//! Price Channel Position indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Position of close within rolling highest-high / lowest-low channel (0-100%).
///
/// 100 = close at period high, 0 = close at period low.
/// Equivalent to a %K stochastic using high/low channel instead of bar extremes.
pub struct PriceChannelPosition {
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl PriceChannelPosition {
    /// Creates a new `PriceChannelPosition` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for PriceChannelPosition {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let highest = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let lowest = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let range = highest - lowest;
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::from(50u32)));
        }
        Ok(SignalValue::Scalar((bar.close - lowest) / range * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.highs.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.highs.clear(); self.lows.clear(); }
    fn name(&self) -> &str { "PriceChannelPosition" }
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
    fn test_price_channel_position_at_high() {
        let mut sig = PriceChannelPosition::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("120", "80", "120")).unwrap(); // close = period high
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_price_channel_position_at_low() {
        let mut sig = PriceChannelPosition::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("120", "80", "80")).unwrap(); // close = period low
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
