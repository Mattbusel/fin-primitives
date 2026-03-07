//! Exponential Moving Average (EMA) indicator.

use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use crate::signals::{Signal, SignalValue};
use rust_decimal::Decimal;

/// Exponential Moving Average over `period` bars.
///
/// Uses an SMA seed for the first `period` bars, then applies:
/// `EMA = close * k + prev_EMA * (1 - k)` where `k = 2 / (period + 1)`.
///
/// Returns `SignalValue::Unavailable` until `period` bars have been seen.
pub struct Ema {
    name: String,
    period: usize,
    current: Option<Decimal>,
    count: usize,
    /// Multiplier: `2 / (period + 1)`
    multiplier: Decimal,
    /// Accumulator for SMA seed phase.
    seed_sum: Decimal,
}

impl Ema {
    /// Constructs a new `Ema` with the given name and period.
    pub fn new(name: impl Into<String>, period: usize) -> Self {
        let denom = Decimal::from((period + 1) as u32);
        let multiplier = Decimal::TWO
            .checked_div(denom)
            .unwrap_or(Decimal::ONE);
        Self {
            name: name.into(),
            period,
            current: None,
            count: 0,
            multiplier,
            seed_sum: Decimal::ZERO,
        }
    }
}

impl Signal for Ema {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &OhlcvBar) -> Result<SignalValue, FinError> {
        let close = bar.close.value();
        self.count += 1;

        if self.count <= self.period {
            // SMA seed phase.
            self.seed_sum += close;
            if self.count == self.period {
                let seed = self.seed_sum
                    .checked_div(Decimal::from(self.period as u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                self.current = Some(seed);
                return Ok(SignalValue::Scalar(seed));
            }
            return Ok(SignalValue::Unavailable);
        }

        // EMA phase.
        let prev = self.current.unwrap_or(Decimal::ZERO);
        let one_minus_k = Decimal::ONE
            .checked_sub(self.multiplier)
            .ok_or(FinError::ArithmeticOverflow)?;
        let ema = close
            .checked_mul(self.multiplier)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_add(prev.checked_mul(one_minus_k).ok_or(FinError::ArithmeticOverflow)?)
            .ok_or(FinError::ArithmeticOverflow)?;
        self.current = Some(ema);
        Ok(SignalValue::Scalar(ema))
    }

    fn is_ready(&self) -> bool {
        self.count >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp(0),
            ts_close: NanoTimestamp(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ema_not_ready_before_period() {
        let mut ema = Ema::new("ema3", 3);
        let v = ema.update(&bar("10")).unwrap();
        assert!(matches!(v, SignalValue::Unavailable));
        assert!(!ema.is_ready());
    }

    #[test]
    fn test_ema_first_value_equals_sma_seed() {
        // period=3: SMA of first 3 bars = (10+20+30)/3 = 20
        let mut ema = Ema::new("ema3", 3);
        ema.update(&bar("10")).unwrap();
        ema.update(&bar("20")).unwrap();
        let v = ema.update(&bar("30")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert_eq!(val, dec!(20));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ema_subsequent_values_weighted() {
        // period=3, k = 2/4 = 0.5
        // seed = (10+20+30)/3 = 20
        // 4th bar close=40: EMA = 40*0.5 + 20*0.5 = 30
        let mut ema = Ema::new("ema3", 3);
        ema.update(&bar("10")).unwrap();
        ema.update(&bar("20")).unwrap();
        ema.update(&bar("30")).unwrap();
        let v = ema.update(&bar("40")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert_eq!(val, dec!(30));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ema_is_ready_after_period() {
        let mut ema = Ema::new("ema3", 3);
        ema.update(&bar("10")).unwrap();
        ema.update(&bar("20")).unwrap();
        assert!(!ema.is_ready());
        ema.update(&bar("30")).unwrap();
        assert!(ema.is_ready());
    }
}
