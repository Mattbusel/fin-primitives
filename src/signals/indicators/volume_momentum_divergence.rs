//! Volume Momentum Divergence indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Difference between rolling volume change and rolling price change direction.
///
/// `sign(volume_change) - sign(price_change)` averaged over the period.
/// Values near +2 or -2 indicate strong divergence (price/volume disagree).
/// Values near 0 indicate convergence (price and volume agree on direction).
pub struct VolumeMomentumDivergence {
    period: usize,
    prev_close: Option<Decimal>,
    prev_volume: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeMomentumDivergence {
    /// Creates a new `VolumeMomentumDivergence` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_close: None,
            prev_volume: None,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeMomentumDivergence {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let (Some(pc), Some(pv)) = (self.prev_close, self.prev_volume) {
            let price_sign: i32 = if bar.close > pc { 1 } else if bar.close < pc { -1 } else { 0 };
            let vol_sign: i32 = if bar.volume > pv { 1 } else if bar.volume < pv { -1 } else { 0 };
            let divergence = Decimal::from(vol_sign - price_sign);
            self.window.push_back(divergence);
            self.sum += divergence;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        self.prev_close = Some(bar.close);
        self.prev_volume = Some(bar.volume);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.prev_volume = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeMomentumDivergence" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str, v: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vmd_convergence() {
        // Price up, volume up → divergence = 0 (both agree)
        let mut sig = VolumeMomentumDivergence::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("101", "1100")).unwrap();
        let v = sig.update(&bar("102", "1200")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vmd_divergence() {
        // Price up (+1), volume down (-1) → vol_sign - price_sign = -2
        let mut sig = VolumeMomentumDivergence::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("101", "900")).unwrap();
        let v = sig.update(&bar("102", "800")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-2)));
    }
}
