//! Double Exponential Moving Average (DEMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Double Exponential Moving Average over `period` bars.
///
/// `DEMA = 2 × EMA(price, n) − EMA(EMA(price, n), n)`
///
/// Reduces lag compared to a plain EMA while remaining smoother than a simple
/// moving average. Both the outer and inner EMA use the same SMA-seeded
/// initialisation as [`crate::signals::indicators::Ema`].
///
/// Returns `SignalValue::Unavailable` until `2 * period - 1` bars have been seen
/// (enough to fully seed both EMA layers).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Dema;
/// use fin_primitives::signals::{Signal, SignalValue};
///
/// let mut dema = Dema::new("dema3", 3).unwrap();
/// // Feed 5 bars — first ready value at bar 5 (2*3-1).
/// ```
pub struct Dema {
    name: String,
    period: usize,
    /// Multiplier k = 2 / (period + 1)
    multiplier: Decimal,
    // --- outer EMA state ---
    outer_count: usize,
    outer_seed_sum: Decimal,
    outer_ema: Option<Decimal>,
    // --- inner EMA-of-EMA state ---
    inner_count: usize,
    inner_seed_sum: Decimal,
    inner_ema: Option<Decimal>,
}

impl Dema {
    /// Constructs a new `Dema` with the given name and period.
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
            outer_count: 0,
            outer_seed_sum: Decimal::ZERO,
            outer_ema: None,
            inner_count: 0,
            inner_seed_sum: Decimal::ZERO,
            inner_ema: None,
        })
    }

    /// Applies one EMA step to `prev` using `value` and the stored multiplier.
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
}

impl Signal for Dema {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        // ── Outer EMA ────────────────────────────────────────────────────────
        self.outer_count += 1;
        let outer_val = if self.outer_count <= self.period {
            self.outer_seed_sum += close;
            if self.outer_count == self.period {
                #[allow(clippy::cast_possible_truncation)]
                let seed = self
                    .outer_seed_sum
                    .checked_div(Decimal::from(self.period as u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                self.outer_ema = Some(seed);
                seed
            } else {
                return Ok(SignalValue::Unavailable);
            }
        } else {
            let prev = self.outer_ema.unwrap_or(Decimal::ZERO);
            let ema = self.ema_step(prev, close)?;
            self.outer_ema = Some(ema);
            ema
        };

        // ── Inner EMA (EMA of outer EMA) ─────────────────────────────────────
        self.inner_count += 1;
        let inner_val = if self.inner_count <= self.period {
            self.inner_seed_sum += outer_val;
            if self.inner_count == self.period {
                #[allow(clippy::cast_possible_truncation)]
                let seed = self
                    .inner_seed_sum
                    .checked_div(Decimal::from(self.period as u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                self.inner_ema = Some(seed);
                seed
            } else {
                return Ok(SignalValue::Unavailable);
            }
        } else {
            let prev = self.inner_ema.unwrap_or(Decimal::ZERO);
            let ema = self.ema_step(prev, outer_val)?;
            self.inner_ema = Some(ema);
            ema
        };

        // DEMA = 2 * outer - inner
        let dema = Decimal::TWO
            .checked_mul(outer_val)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_sub(inner_val)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(dema))
    }

    fn is_ready(&self) -> bool {
        // Need period bars for outer EMA + period bars for inner EMA,
        // but they overlap by 1 (when outer first becomes ready, inner starts).
        self.outer_count >= self.period && self.inner_count >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.outer_count = 0;
        self.outer_seed_sum = Decimal::ZERO;
        self.outer_ema = None;
        self.inner_count = 0;
        self.inner_seed_sum = Decimal::ZERO;
        self.inner_ema = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
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
    fn test_dema_period_0_error() {
        assert!(Dema::new("d", 0).is_err());
    }

    #[test]
    fn test_dema_unavailable_before_ready() {
        let mut dema = Dema::new("d3", 3).unwrap();
        // Needs 2*3-1 = 5 bars before first ready value
        for _ in 0..4 {
            assert_eq!(dema.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!dema.is_ready());
    }

    #[test]
    fn test_dema_ready_at_2_period_minus_1() {
        let mut dema = Dema::new("d3", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 {
            last = dema.update_bar(&bar("100")).unwrap();
        }
        assert!(dema.is_ready());
        assert!(matches!(last, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_dema_constant_price_equals_price() {
        // For constant price p, both EMA layers equal p, so DEMA = 2p - p = p.
        let mut dema = Dema::new("d3", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 {
            last = dema.update_bar(&bar("50")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar("50".parse().unwrap()));
    }

    #[test]
    fn test_dema_reset_clears_state() {
        let mut dema = Dema::new("d3", 3).unwrap();
        for _ in 0..5 {
            dema.update_bar(&bar("100")).unwrap();
        }
        assert!(dema.is_ready());
        dema.reset();
        assert!(!dema.is_ready());
        assert_eq!(dema.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_dema_reacts_faster_than_ema_to_jump() {
        use crate::signals::indicators::Ema;
        let period = 5;
        let mut dema = Dema::new("d5", period).unwrap();
        let mut ema = Ema::new("e5", period).unwrap();

        // Warm up with constant price
        for _ in 0..(2 * period - 1) {
            dema.update_bar(&bar("100")).unwrap();
        }
        for _ in 0..period {
            ema.update_bar(&bar("100")).unwrap();
        }

        // Apply a price jump
        let dema_val = match dema.update_bar(&bar("200")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("DEMA should be ready"),
        };
        let ema_val = match ema.update_bar(&bar("200")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("EMA should be ready"),
        };

        assert!(
            dema_val > ema_val,
            "DEMA ({dema_val}) should react faster than EMA ({ema_val}) to a price jump"
        );
    }
}
