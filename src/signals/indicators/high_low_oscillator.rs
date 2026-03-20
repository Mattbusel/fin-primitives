//! High-Low Oscillator indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// `(period_high + period_low) / 2 - SMA(close, period)`.
///
/// Measures how close the SMA is to the channel midpoint.
/// Positive: SMA above channel midpoint (upper channel bias).
/// Negative: SMA below channel midpoint (lower channel bias).
pub struct HighLowOscillator {
    period: usize,
    window: VecDeque<BarInput>,
    close_sum: Decimal,
}

impl HighLowOscillator {
    /// Creates a new `HighLowOscillator` with the given rolling period.
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

impl Signal for HighLowOscillator {
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

        let period_high = self.window.iter().map(|b| b.high).fold(Decimal::MIN, Decimal::max);
        let period_low = self.window.iter().map(|b| b.low).fold(Decimal::MAX, Decimal::min);
        let channel_mid = (period_high + period_low) / Decimal::TWO;
        let sma = self.close_sum / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(sma - channel_mid))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.close_sum = Decimal::ZERO; }
    fn name(&self) -> &str { "HighLowOscillator" }
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
    fn test_hlo_close_at_midpoint() {
        // SMA = channel midpoint → oscillator = 0
        let mut sig = HighLowOscillator::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("110", "90", "100")).unwrap();
        // channel_mid = (110+90)/2 = 100, sma = 100 → diff = 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hlo_close_above_midpoint() {
        let mut sig = HighLowOscillator::new(2).unwrap();
        sig.update(&bar("110", "90", "108")).unwrap();
        let v = sig.update(&bar("110", "90", "108")).unwrap();
        // sma=108, channel_mid=100 → diff=8
        assert_eq!(v, SignalValue::Scalar(dec!(8)));
    }
}
