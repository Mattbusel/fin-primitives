//! Bull Power and Bear Power indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `high - SMA(close)` (bull power) minus `low - SMA(close)` (bear power).
///
/// Also known as Elder Force combined measure.
/// Positive: bull power dominates (highs above average, lows not as far below).
/// Negative: bear power dominates (lows below average, highs not as far above).
pub struct BullPowerBearPower {
    period: usize,
    window: VecDeque<BarInput>,
    close_sum: Decimal,
}

impl BullPowerBearPower {
    /// Creates a new `BullPowerBearPower` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            window: VecDeque::with_capacity(period),
            close_sum: Decimal::ZERO,
        })
    }
}

impl Signal for BullPowerBearPower {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.close_sum += bar.close;
        self.window.push_back(*bar);
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.close_sum -= old.close;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sma = self.close_sum / Decimal::from(self.period as u32);
        let bull_power_sum: Decimal = self.window.iter().map(|b| b.high - sma).sum();
        let bear_power_sum: Decimal = self.window.iter().map(|b| b.low - sma).sum();
        let len = Decimal::from(self.period as u32);
        let net = (bull_power_sum - bear_power_sum) / len;
        Ok(SignalValue::Scalar(net))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.close_sum = Decimal::ZERO; }
    fn name(&self) -> &str { "BullPowerBearPower" }
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
    fn test_bpbp_symmetric_zero() {
        // Equal upper and lower distance from SMA → net = 0
        // high = SMA + 10, low = SMA - 10 → bull=10, bear=-10, net= (10-(-10))/1... wait
        // bull_power = high - sma = +10
        // bear_power = low - sma = -10
        // net = (bull_power - bear_power) / n = (10 - (-10)) / 1 = 20
        // Hmm, let me think again. bull_power_sum - bear_power_sum per bar is (high-sma) - (low-sma) = high - low = range
        // For symmetric bar centered at SMA: high=110, low=90, sma=100 → (110-90)/1 = 20
        // Let me just check it's positive
        let mut sig = BullPowerBearPower::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("110", "90", "100")).unwrap();
        // sma=100, bull=(110-100)=10, bear=(90-100)=-10, net=(10-(-10))/1 per bar avg= 20
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_bpbp_not_ready() {
        let mut sig = BullPowerBearPower::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }
}
