//! Up Volume Fraction indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling fraction of total volume on up bars: `sum(up_volume) / sum(all_volume)`.
///
/// Values near 1.0: most volume transacted on up bars (accumulation/bullish).
/// Values near 0.0: most volume transacted on down bars (distribution/bearish).
/// Values near 0.5: balanced volume distribution.
pub struct UpVolumeFraction {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<(Decimal, Decimal)>, // (up_vol, total_vol)
    up_sum: Decimal,
    total_sum: Decimal,
}

impl UpVolumeFraction {
    /// Creates a new `UpVolumeFraction` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
            up_sum: Decimal::ZERO,
            total_sum: Decimal::ZERO,
        })
    }
}

impl Signal for UpVolumeFraction {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let up_vol = if bar.close > pc { bar.volume } else { Decimal::ZERO };
            self.window.push_back((up_vol, bar.volume));
            self.up_sum += up_vol;
            self.total_sum += bar.volume;
            if self.window.len() > self.period {
                if let Some((old_up, old_total)) = self.window.pop_front() {
                    self.up_sum -= old_up;
                    self.total_sum -= old_total;
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.total_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.up_sum / self.total_sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
        self.up_sum = Decimal::ZERO;
        self.total_sum = Decimal::ZERO;
    }
    fn name(&self) -> &str { "UpVolumeFraction" }
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
    fn test_uvf_all_up() {
        // All up bars → fraction = 1
        let mut sig = UpVolumeFraction::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("101", "1000")).unwrap();
        let v = sig.update(&bar("102", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_uvf_all_down() {
        // All down bars → up_vol = 0 → fraction = 0
        let mut sig = UpVolumeFraction::new(2).unwrap();
        sig.update(&bar("102", "1000")).unwrap();
        sig.update(&bar("101", "1000")).unwrap();
        let v = sig.update(&bar("100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
