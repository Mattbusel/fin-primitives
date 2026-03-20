//! Close-Relative-to-EMA indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Percentage deviation of close from its EMA: `(close - EMA) / EMA * 100`.
///
/// Positive values indicate close is above EMA (bullish extension).
/// Negative values indicate close is below EMA (bearish compression).
pub struct CloseRelativeToEma {
    period: usize,
    k: Decimal,
    ema: Option<Decimal>,
    bars_seen: usize,
}

impl CloseRelativeToEma {
    /// Creates a new `CloseRelativeToEma` with the given EMA period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let k = Decimal::TWO / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { period, k, ema: None, bars_seen: 0 })
    }
}

impl Signal for CloseRelativeToEma {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ema = match self.ema {
            None => bar.close,
            Some(prev) => bar.close * self.k + prev * (Decimal::ONE - self.k),
        };
        self.ema = Some(ema);
        self.bars_seen += 1;
        if self.bars_seen < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if ema.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar((bar.close - ema) / ema * Decimal::ONE_HUNDRED))
    }

    fn is_ready(&self) -> bool { self.bars_seen >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.ema = None; self.bars_seen = 0; }
    fn name(&self) -> &str { "CloseRelativeToEma" }
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
    fn test_cre_not_ready() {
        let mut sig = CloseRelativeToEma::new(3).unwrap();
        assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cre_constant_close_zero_deviation() {
        // After warm-up with constant close, EMA = close → deviation = 0
        let mut sig = CloseRelativeToEma::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
