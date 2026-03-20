//! Zero-Lag EMA (ZLEMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Zero-Lag Exponential Moving Average over `period` bars.
///
/// Reduces EMA lag by feeding `2 × close - close[lag]` into the EMA, where
/// `lag = (period - 1) / 2`.
///
/// Returns [`SignalValue::Unavailable`] until enough bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Zlema;
/// use fin_primitives::signals::Signal;
///
/// let mut z = Zlema::new("zlema20", 20).unwrap();
/// assert_eq!(z.period(), 20);
/// ```
pub struct Zlema {
    name: String,
    period: usize,
    lag: usize,
    k: Decimal,
    history: VecDeque<Decimal>,
    ema: Option<Decimal>,
    seed_sum: Decimal,
    seed_count: usize,
}

impl Zlema {
    /// Constructs a new `Zlema`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        let lag = (period - 1) / 2;
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::TWO / Decimal::from((period + 1) as u32);
        Ok(Self {
            name: name.into(),
            period,
            lag,
            k,
            history: VecDeque::with_capacity(lag + 1),
            ema: None,
            seed_sum: Decimal::ZERO,
            seed_count: 0,
        })
    }
}

impl Signal for Zlema {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() > self.lag + 1 {
            self.history.pop_front();
        }

        // Adjusted input: 2*close - close[lag] (or just close if not enough history)
        let lagged = if self.history.len() > self.lag {
            *self.history.front().unwrap()
        } else {
            bar.close
        };
        let adjusted = Decimal::TWO * bar.close - lagged;

        self.seed_count += 1;
        if self.seed_count <= self.period {
            self.seed_sum += adjusted;
            if self.seed_count == self.period {
                #[allow(clippy::cast_possible_truncation)]
                let seed = self.seed_sum / Decimal::from(self.period as u32);
                self.ema = Some(seed);
                return Ok(SignalValue::Scalar(seed));
            }
            return Ok(SignalValue::Unavailable);
        }
        let prev = self.ema.unwrap_or(adjusted);
        let new_ema = adjusted * self.k + prev * (Decimal::ONE - self.k);
        self.ema = Some(new_ema);
        Ok(SignalValue::Scalar(new_ema))
    }

    fn is_ready(&self) -> bool {
        self.ema.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.history.clear();
        self.ema = None;
        self.seed_sum = Decimal::ZERO;
        self.seed_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_zlema_period_0_error() {
        assert!(Zlema::new("z", 0).is_err());
    }

    #[test]
    fn test_zlema_unavailable_before_period() {
        let mut z = Zlema::new("z3", 3).unwrap();
        assert_eq!(z.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(z.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(z.update_bar(&bar("102")).unwrap().is_scalar());
    }

    #[test]
    fn test_zlema_constant_price_equals_price() {
        let mut z = Zlema::new("z3", 3).unwrap();
        for _ in 0..10 {
            z.update_bar(&bar("100")).unwrap();
        }
        match z.update_bar(&bar("100")).unwrap() {
            SignalValue::Scalar(d) => assert_eq!(d, dec!(100)),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn test_zlema_reset() {
        let mut z = Zlema::new("z3", 3).unwrap();
        for _ in 0..5 { z.update_bar(&bar("100")).unwrap(); }
        assert!(z.is_ready());
        z.reset();
        assert!(!z.is_ready());
    }
}
