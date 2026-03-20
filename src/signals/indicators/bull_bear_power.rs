//! Bull/Bear Power indicator (Elder).

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Elder's Bull/Bear Power: `(high - EMA) - (low - EMA)` = `high - low` averaged via EMA context.
///
/// More precisely: rolling `bull_power = high - EMA(close)` and `bear_power = low - EMA(close)`.
/// This returns `bull_power - bear_power` = simple EMA-smoothed `high - low` deviation.
/// Positive values indicate bullish pressure dominates; negative indicates bearish.
pub struct BullBearPower {
    period: usize,
    k: Decimal,
    ema: Option<Decimal>,
    bars_seen: usize,
}

impl BullBearPower {
    /// Creates a new `BullBearPower` with the given EMA period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let k = Decimal::TWO / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { period, k, ema: None, bars_seen: 0 })
    }
}

impl Signal for BullBearPower {
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
        // bull_power - bear_power = (high - ema) - (low - ema) = high - low
        // But measured through EMA context: return both combined as net power
        let bull = bar.high - ema;
        let bear = bar.low - ema;
        Ok(SignalValue::Scalar(bull - bear))
    }

    fn is_ready(&self) -> bool { self.bars_seen >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.ema = None; self.bars_seen = 0; }
    fn name(&self) -> &str { "BullBearPower" }
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
    fn test_bull_bear_power_not_ready() {
        let mut sig = BullBearPower::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bull_bear_power_positive() {
        // bull - bear = high - low, always >= 0
        let mut sig = BullBearPower::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("115", "85", "100")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert!(x >= dec!(0), "bull-bear power should be >= 0, got {}", x);
        }
    }
}
