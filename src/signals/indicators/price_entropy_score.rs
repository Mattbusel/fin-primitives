//! Price Entropy Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Approximate entropy of close returns over the rolling window.
///
/// Discretizes returns into bins (up/flat/down) and computes Shannon entropy.
/// High entropy: unpredictable, random-walk-like market.
/// Low entropy: predictable, trending or mean-reverting regime.
/// Entropy is normalized to [0, 1] by dividing by log2(3).
pub struct PriceEntropyScore {
    period: usize,
    prev_close: Option<Decimal>,
    signs: VecDeque<i8>, // -1, 0, +1
}

impl PriceEntropyScore {
    /// Creates a new `PriceEntropyScore` with the given rolling period (min 3).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 3 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, signs: VecDeque::with_capacity(period) })
    }
}

impl Signal for PriceEntropyScore {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let sign: i8 = if bar.close > pc { 1 } else if bar.close < pc { -1 } else { 0 };
            self.signs.push_back(sign);
            if self.signs.len() > self.period {
                self.signs.pop_front();
            }
        }
        self.prev_close = Some(bar.close);

        if self.signs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let up = self.signs.iter().filter(|&&s| s == 1).count() as f64;
        let down = self.signs.iter().filter(|&&s| s == -1).count() as f64;
        let flat = self.signs.iter().filter(|&&s| s == 0).count() as f64;

        let mut entropy = 0.0f64;
        for &count in &[up, down, flat] {
            if count > 0.0 {
                let p = count / n;
                entropy -= p * p.log2();
            }
        }
        // normalize by log2(3) ≈ 1.585
        let normalized = entropy / std::f64::consts::LOG2_E.recip().mul_add(3.0f64.ln(), 0.0);
        // simpler: log2(3) = ln(3)/ln(2)
        let log2_3 = 3.0f64.ln() / 2.0f64.ln();
        let normalized = entropy / log2_3;

        match Decimal::from_f64_retain(normalized) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.signs.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.signs.clear(); }
    fn name(&self) -> &str { "PriceEntropyScore" }
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
    fn test_pes_all_same_direction_low_entropy() {
        // All up → only one bin populated → entropy = 0
        let mut sig = PriceEntropyScore::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pes_mixed_higher_entropy() {
        // Mix of up and down → higher entropy
        let mut sig = PriceEntropyScore::new(4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap(); // up
        sig.update(&bar("100")).unwrap(); // down
        sig.update(&bar("102")).unwrap(); // up
        if let SignalValue::Scalar(v) = sig.update(&bar("100")).unwrap() { // down
            // window=[up,down,up,down], entropy > 0
            assert!(v > dec!(0), "expected non-zero entropy, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
