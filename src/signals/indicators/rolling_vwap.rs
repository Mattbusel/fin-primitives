//! Rolling VWAP indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling Volume-Weighted Average Price over N bars.
///
/// `Σ(typical_price * volume) / Σ(volume)` where `typical_price = (high + low + close) / 3`.
/// Tracks where price has been on a volume-weighted basis.
/// Close above: bullish (price trading above avg cost basis).
/// Close below: bearish (price below avg cost basis).
pub struct RollingVwap {
    period: usize,
    window: VecDeque<(Decimal, Decimal)>, // (tp * vol, vol)
    pv_sum: Decimal,
    vol_sum: Decimal,
}

impl RollingVwap {
    /// Creates a new `RollingVwap` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            window: VecDeque::with_capacity(period),
            pv_sum: Decimal::ZERO,
            vol_sum: Decimal::ZERO,
        })
    }
}

impl Signal for RollingVwap {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let three = Decimal::from(3u32);
        let tp = (bar.high + bar.low + bar.close) / three;
        let pv = tp * bar.volume;

        self.window.push_back((pv, bar.volume));
        self.pv_sum += pv;
        self.vol_sum += bar.volume;

        if self.window.len() > self.period {
            if let Some((old_pv, old_vol)) = self.window.pop_front() {
                self.pv_sum -= old_pv;
                self.vol_sum -= old_vol;
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.vol_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.pv_sum / self.vol_sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.window.clear();
        self.pv_sum = Decimal::ZERO;
        self.vol_sum = Decimal::ZERO;
    }
    fn name(&self) -> &str { "RollingVwap" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str, v: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_rvwap_equal_volume_is_avg_tp() {
        // Equal volume → VWAP = avg of typical prices
        // tp1 = (110+90+100)/3 = 100, tp2 = (110+90+100)/3 = 100
        let mut sig = RollingVwap::new(2).unwrap();
        sig.update(&bar("110", "90", "100", "1000")).unwrap();
        let v = sig.update(&bar("110", "90", "100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_rvwap_higher_volume_weights_more() {
        // Bar1: tp=90, vol=1; Bar2: tp=110, vol=9 → VWAP close to 110
        let mut sig = RollingVwap::new(2).unwrap();
        sig.update(&bar("90", "90", "90", "1")).unwrap();   // tp=90
        if let SignalValue::Scalar(v) = sig.update(&bar("110", "110", "110", "9")).unwrap() { // tp=110
            // VWAP = (90*1 + 110*9) / 10 = (90+990)/10 = 108
            assert!(v > dec!(100), "high-vol bar should pull VWAP above 100, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
