//! Volatility Spike indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Returns 1 if current bar range exceeds N-bar rolling average by a multiplier, else 0.
///
/// Detects sudden volatility spikes relative to recent baseline.
/// Useful for flagging news events, gaps, or exceptional market activity.
/// `multiplier` is provided as a percentage integer (e.g., 200 = 2x average range).
pub struct VolatilitySpike {
    period: usize,
    multiplier_pct: u32,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolatilitySpike {
    /// Creates a new `VolatilitySpike`.
    ///
    /// `multiplier_pct`: threshold as % of average range (e.g., 200 = 2x, 150 = 1.5x).
    pub fn new(period: usize, multiplier_pct: u32) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            multiplier_pct,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolatilitySpike {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.window.push_back(range);
        self.sum += range;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg_range = self.sum / Decimal::from(self.period as u32);
        let threshold = avg_range * Decimal::from(self.multiplier_pct) / Decimal::ONE_HUNDRED;
        let spike: i32 = if range > threshold { 1 } else { 0 };
        Ok(SignalValue::Scalar(Decimal::from(spike)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolatilitySpike" }
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
    fn test_vs_no_spike() {
        // All bars same range → current = avg → no spike (not strictly greater)
        let mut sig = VolatilitySpike::new(3, 150).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vs_spike_detected() {
        // Previous bars range=20, current range=60 → 60 > 20*1.5=30 → spike
        let mut sig = VolatilitySpike::new(3, 150).unwrap();
        sig.update(&bar("110", "90")).unwrap(); // range=20
        sig.update(&bar("110", "90")).unwrap(); // range=20
        sig.update(&bar("110", "90")).unwrap(); // range=20, avg=20
        let v = sig.update(&bar("130", "70")).unwrap(); // range=60 > 20*1.5=30 → spike
        // but window now includes this 60 too: avg=(20+20+60)/3=33.3, threshold=50
        // Actually window slides: [20,20,60], avg=33.3, threshold=50 → 60>50 → spike
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }
}
