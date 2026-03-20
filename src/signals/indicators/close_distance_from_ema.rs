//! Close Distance From EMA indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Signed distance of close from its EMA: `(close - EMA) / EMA * 100`.
///
/// Positive: close above EMA (bullish momentum).
/// Negative: close below EMA (bearish momentum).
/// Uses standard EMA smoothing: `k = 2 / (period + 1)`.
pub struct CloseDistanceFromEma {
    period: usize,
    k: Decimal,
    ema: Option<Decimal>,
    warm_up: usize,
    warm_up_sum: Decimal,
}

impl CloseDistanceFromEma {
    /// Creates a new `CloseDistanceFromEma` with the given smoothing period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let k = Decimal::TWO / Decimal::from((period + 1) as u32);
        Ok(Self { period, k, ema: None, warm_up: 0, warm_up_sum: Decimal::ZERO })
    }
}

impl Signal for CloseDistanceFromEma {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ema = match self.ema {
            None => {
                self.warm_up_sum += bar.close;
                self.warm_up += 1;
                if self.warm_up >= self.period {
                    let seed = self.warm_up_sum / Decimal::from(self.period as u32);
                    self.ema = Some(seed);
                    seed
                } else {
                    return Ok(SignalValue::Unavailable);
                }
            }
            Some(prev) => {
                let new_ema = bar.close * self.k + prev * (Decimal::ONE - self.k);
                self.ema = Some(new_ema);
                new_ema
            }
        };

        if ema.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar((bar.close - ema) / ema * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.ema.is_some() }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.ema = None; self.warm_up = 0; self.warm_up_sum = Decimal::ZERO; }
    fn name(&self) -> &str { "CloseDistanceFromEma" }
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
    fn test_cdfe_seed_bar_zero_distance() {
        // After warmup, close = EMA → distance = 0
        let mut sig = CloseDistanceFromEma::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap(); // EMA=100, close=100 → 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cdfe_above_ema_positive() {
        // Price surges above EMA → positive distance
        let mut sig = CloseDistanceFromEma::new(2).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap(); // EMA seeded at 100
        if let SignalValue::Scalar(v) = sig.update(&bar("120")).unwrap() {
            // EMA updated: 120*(2/3) + 100*(1/3) = 80+33.33 = 113.33..., distance = (120-113.33)/113.33*100 > 0
            assert!(v > dec!(0), "expected positive, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }
}
