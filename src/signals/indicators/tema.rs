//! Triple Exponential Moving Average (TEMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Triple Exponential Moving Average over `period` bars.
///
/// `TEMA = 3×EMA₁ − 3×EMA₂ + EMA₃`
///
/// where `EMA₁ = EMA(price, n)`, `EMA₂ = EMA(EMA₁, n)`, `EMA₃ = EMA(EMA₂, n)`.
///
/// TEMA reduces lag further than [`crate::signals::indicators::Dema`] at the cost of
/// requiring `3*period − 2` bars to warm up.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Tema;
/// use fin_primitives::signals::Signal;
///
/// let mut tema = Tema::new("tema3", 3).unwrap();
/// // Ready after 2*3-1 = 5 bars ... actually 3*3-2 = 7 bars
/// ```
pub struct Tema {
    name: String,
    period: usize,
    multiplier: Decimal,
    // EMA₁
    e1_count: usize,
    e1_seed_sum: Decimal,
    e1: Option<Decimal>,
    // EMA₂
    e2_count: usize,
    e2_seed_sum: Decimal,
    e2: Option<Decimal>,
    // EMA₃
    e3_count: usize,
    e3_seed_sum: Decimal,
    e3: Option<Decimal>,
}

impl Tema {
    /// Constructs a new `Tema` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let denom = Decimal::from((period + 1) as u32);
        let multiplier = Decimal::TWO.checked_div(denom).unwrap_or(Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            e1_count: 0,
            e1_seed_sum: Decimal::ZERO,
            e1: None,
            e2_count: 0,
            e2_seed_sum: Decimal::ZERO,
            e2: None,
            e3_count: 0,
            e3_seed_sum: Decimal::ZERO,
            e3: None,
        })
    }

    fn ema_step(&self, prev: Decimal, value: Decimal) -> Result<Decimal, FinError> {
        let one_minus_k = Decimal::ONE
            .checked_sub(self.multiplier)
            .ok_or(FinError::ArithmeticOverflow)?;
        value
            .checked_mul(self.multiplier)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_add(
                prev.checked_mul(one_minus_k)
                    .ok_or(FinError::ArithmeticOverflow)?,
            )
            .ok_or(FinError::ArithmeticOverflow)
    }

    fn ema_update(
        count: &mut usize,
        seed_sum: &mut Decimal,
        state: &mut Option<Decimal>,
        period: usize,
        multiplier: Decimal,
        value: Decimal,
    ) -> Result<Option<Decimal>, FinError> {
        *count += 1;
        if *count <= period {
            *seed_sum += value;
            if *count == period {
                #[allow(clippy::cast_possible_truncation)]
                let seed = seed_sum
                    .checked_div(Decimal::from(period as u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                *state = Some(seed);
                return Ok(Some(seed));
            }
            return Ok(None);
        }
        let prev = state.unwrap_or(Decimal::ZERO);
        let one_minus_k = Decimal::ONE
            .checked_sub(multiplier)
            .ok_or(FinError::ArithmeticOverflow)?;
        let ema = value
            .checked_mul(multiplier)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_add(
                prev.checked_mul(one_minus_k)
                    .ok_or(FinError::ArithmeticOverflow)?,
            )
            .ok_or(FinError::ArithmeticOverflow)?;
        *state = Some(ema);
        Ok(Some(ema))
    }
}

impl Signal for Tema {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        let k = self.multiplier;
        let p = self.period;

        // EMA₁
        let e1_val = match Self::ema_update(
            &mut self.e1_count,
            &mut self.e1_seed_sum,
            &mut self.e1,
            p,
            k,
            close,
        )? {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };

        // EMA₂
        let e2_val = match Self::ema_update(
            &mut self.e2_count,
            &mut self.e2_seed_sum,
            &mut self.e2,
            p,
            k,
            e1_val,
        )? {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };

        // EMA₃
        let e3_val = match Self::ema_update(
            &mut self.e3_count,
            &mut self.e3_seed_sum,
            &mut self.e3,
            p,
            k,
            e2_val,
        )? {
            Some(v) => v,
            None => return Ok(SignalValue::Unavailable),
        };

        // TEMA = 3*e1 - 3*e2 + e3
        let three = Decimal::from(3u32);
        let tema = three
            .checked_mul(e1_val)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_sub(
                three
                    .checked_mul(e2_val)
                    .ok_or(FinError::ArithmeticOverflow)?,
            )
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_add(e3_val)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(tema))
    }

    fn is_ready(&self) -> bool {
        self.e1_count >= self.period
            && self.e2_count >= self.period
            && self.e3_count >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.e1_count = 0;
        self.e1_seed_sum = Decimal::ZERO;
        self.e1 = None;
        self.e2_count = 0;
        self.e2_seed_sum = Decimal::ZERO;
        self.e2 = None;
        self.e3_count = 0;
        self.e3_seed_sum = Decimal::ZERO;
        self.e3 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::indicators::{Dema, Ema};
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_tema_period_0_error() {
        assert!(Tema::new("t", 0).is_err());
    }

    #[test]
    fn test_tema_constant_price_equals_price() {
        let mut tema = Tema::new("t3", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..12 {
            last = tema.update_bar(&bar("60")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar("60".parse().unwrap()));
    }

    #[test]
    fn test_tema_reset_clears_state() {
        let mut tema = Tema::new("t3", 3).unwrap();
        for _ in 0..10 {
            tema.update_bar(&bar("100")).unwrap();
        }
        assert!(tema.is_ready());
        tema.reset();
        assert!(!tema.is_ready());
        assert_eq!(tema.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_tema_faster_than_dema_on_jump() {
        let period = 3;
        let warmup = 3 * period;
        let mut tema = Tema::new("t3", period).unwrap();
        let mut dema = Dema::new("d3", period).unwrap();
        let mut ema = Ema::new("e3", period).unwrap();

        for _ in 0..warmup {
            tema.update_bar(&bar("100")).unwrap();
            dema.update_bar(&bar("100")).unwrap();
        }
        for _ in 0..period {
            ema.update_bar(&bar("100")).unwrap();
        }

        let tema_v = match tema.update_bar(&bar("500")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("TEMA should be ready"),
        };
        let dema_v = match dema.update_bar(&bar("500")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("DEMA should be ready"),
        };
        let ema_v = match ema.update_bar(&bar("500")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("EMA should be ready"),
        };

        assert!(
            tema_v > dema_v && dema_v > ema_v,
            "TEMA ({tema_v}) > DEMA ({dema_v}) > EMA ({ema_v}) on price jump"
        );
    }
}
