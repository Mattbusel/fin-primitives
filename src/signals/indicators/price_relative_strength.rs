//! Price Relative Strength indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Ratio of average gains to average losses over rolling period (RSI numerator component).
///
/// `avg_gain / avg_loss` — the raw RS value used in RSI.
/// High values: gains dominate (strong upward momentum).
/// Low values: losses dominate (strong downward momentum).
/// Returns MAX_VALUE when avg_loss is zero (pure bullish streak).
pub struct PriceRelativeStrength {
    period: usize,
    prev_close: Option<Decimal>,
    gain_window: VecDeque<Decimal>,
    loss_window: VecDeque<Decimal>,
    gain_sum: Decimal,
    loss_sum: Decimal,
}

impl PriceRelativeStrength {
    /// Creates a new `PriceRelativeStrength` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_close: None,
            gain_window: VecDeque::with_capacity(period),
            loss_window: VecDeque::with_capacity(period),
            gain_sum: Decimal::ZERO,
            loss_sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceRelativeStrength {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let gain = if bar.close > pc { bar.close - pc } else { Decimal::ZERO };
            let loss = if bar.close < pc { pc - bar.close } else { Decimal::ZERO };

            self.gain_window.push_back(gain);
            self.loss_window.push_back(loss);
            self.gain_sum += gain;
            self.loss_sum += loss;

            if self.gain_window.len() > self.period {
                if let Some(og) = self.gain_window.pop_front() { self.gain_sum -= og; }
                if let Some(ol) = self.loss_window.pop_front() { self.loss_sum -= ol; }
            }
        }
        self.prev_close = Some(bar.close);

        if self.gain_window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg_gain = self.gain_sum / Decimal::from(self.period as u32);
        let avg_loss = self.loss_sum / Decimal::from(self.period as u32);

        if avg_loss.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::from(u32::MAX)));
        }
        Ok(SignalValue::Scalar(avg_gain / avg_loss))
    }

    fn is_ready(&self) -> bool { self.gain_window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.prev_close = None;
        self.gain_window.clear();
        self.loss_window.clear();
        self.gain_sum = Decimal::ZERO;
        self.loss_sum = Decimal::ZERO;
    }
    fn name(&self) -> &str { "PriceRelativeStrength" }
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
    fn test_prs_equal_gains_losses_one() {
        // Average gain = average loss → RS = 1
        let mut sig = PriceRelativeStrength::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap(); // gain=2
        let v = sig.update(&bar("100")).unwrap(); // loss=2, avg_gain=1, avg_loss=1 → RS=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_prs_all_up_max() {
        // All gains → avg_loss=0 → returns MAX
        let mut sig = PriceRelativeStrength::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("104")).unwrap();
        if let SignalValue::Scalar(rs) = v {
            assert!(rs > dec!(1000), "expected large RS for all-up, got {rs}");
        } else {
            panic!("expected Scalar");
        }
    }
}
