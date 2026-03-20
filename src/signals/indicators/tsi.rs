//! True Strength Index (TSI) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// True Strength Index (TSI) — a double-smoothed momentum oscillator.
///
/// ```text
/// momentum = close - prev_close
/// TSI = 100 × EMA(EMA(momentum, fast), slow) / EMA(EMA(|momentum|, fast), slow)
/// ```
///
/// Typical parameters: `slow = 25`, `fast = 13`.
/// Output range is approximately `(-100, 100)`:
/// - Positive values indicate bullish momentum
/// - Negative values indicate bearish momentum
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Tsi;
/// use fin_primitives::signals::Signal;
///
/// let tsi = Tsi::new("tsi", 25, 13).unwrap();
/// assert_eq!(tsi.period(), 25);
/// ```
pub struct Tsi {
    name: String,
    slow: usize,
    fast: usize,
    prev_close: Option<Decimal>,
    // EMA state for double-smoothed momentum
    m_slow: Option<Decimal>,
    m_fast: Option<Decimal>,
    // EMA state for double-smoothed absolute momentum
    am_slow: Option<Decimal>,
    am_fast: Option<Decimal>,
    slow_k: Decimal,
    fast_k: Decimal,
    slow_count: usize,
    slow_m_sum: Decimal,
    slow_am_sum: Decimal,
    fast_m_sum: Option<Decimal>,
    fast_am_sum: Option<Decimal>,
    fast_count: usize,
}

impl Tsi {
    /// Constructs a new `Tsi` with `slow` and `fast` EMA periods.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `slow == 0` or `fast == 0`.
    pub fn new(name: impl Into<String>, slow: usize, fast: usize) -> Result<Self, FinError> {
        if slow == 0 {
            return Err(FinError::InvalidPeriod(slow));
        }
        if fast == 0 {
            return Err(FinError::InvalidPeriod(fast));
        }
        #[allow(clippy::cast_possible_truncation)]
        let slow_k = Decimal::TWO / Decimal::from((slow + 1) as u32);
        #[allow(clippy::cast_possible_truncation)]
        let fast_k = Decimal::TWO / Decimal::from((fast + 1) as u32);
        Ok(Self {
            name: name.into(),
            slow,
            fast,
            prev_close: None,
            m_slow: None,
            m_fast: None,
            am_slow: None,
            am_fast: None,
            slow_k,
            fast_k,
            slow_count: 0,
            slow_m_sum: Decimal::ZERO,
            slow_am_sum: Decimal::ZERO,
            fast_m_sum: None,
            fast_am_sum: None,
            fast_count: 0,
        })
    }

    fn ema_step(prev: Decimal, new_val: Decimal, k: Decimal) -> Decimal {
        new_val * k + prev * (Decimal::ONE - k)
    }
}

impl Signal for Tsi {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        let Some(pc) = self.prev_close else {
            self.prev_close = Some(close);
            return Ok(SignalValue::Unavailable);
        };
        self.prev_close = Some(close);

        let mom = close - pc;
        let abs_mom = mom.abs();

        self.slow_count += 1;

        // Seed the slow EMA
        if self.slow_count <= self.slow {
            self.slow_m_sum += mom;
            self.slow_am_sum += abs_mom;
            if self.slow_count < self.slow {
                return Ok(SignalValue::Unavailable);
            }
            // First slow EMA value = SMA seed
            #[allow(clippy::cast_possible_truncation)]
            let denom = Decimal::from(self.slow as u32);
            let m_seed = self.slow_m_sum / denom;
            let am_seed = self.slow_am_sum / denom;
            self.m_slow = Some(m_seed);
            self.am_slow = Some(am_seed);
            // Seed the fast EMA from this first slow value
            self.fast_m_sum = Some(m_seed);
            self.fast_am_sum = Some(am_seed);
            self.fast_count = 1;
            return Ok(SignalValue::Unavailable);
        }

        // Subsequent slow EMA
        let m_slow_prev = self.m_slow.unwrap_or(Decimal::ZERO);
        let am_slow_prev = self.am_slow.unwrap_or(Decimal::ZERO);
        let m_slow = Self::ema_step(m_slow_prev, mom, self.slow_k);
        let am_slow = Self::ema_step(am_slow_prev, abs_mom, self.slow_k);
        self.m_slow = Some(m_slow);
        self.am_slow = Some(am_slow);

        // Seed/apply fast EMA on the slow EMA output
        self.fast_count += 1;
        if self.fast_count <= self.fast {
            *self.fast_m_sum.get_or_insert(Decimal::ZERO) += m_slow;
            *self.fast_am_sum.get_or_insert(Decimal::ZERO) += am_slow;
            if self.fast_count < self.fast {
                return Ok(SignalValue::Unavailable);
            }
            #[allow(clippy::cast_possible_truncation)]
            let denom = Decimal::from(self.fast as u32);
            self.m_fast = Some(self.fast_m_sum.unwrap_or(Decimal::ZERO) / denom);
            self.am_fast = Some(self.fast_am_sum.unwrap_or(Decimal::ZERO) / denom);
        } else {
            let m_fast = Self::ema_step(self.m_fast.unwrap_or(m_slow), m_slow, self.fast_k);
            let am_fast = Self::ema_step(self.am_fast.unwrap_or(am_slow), am_slow, self.fast_k);
            self.m_fast = Some(m_fast);
            self.am_fast = Some(am_fast);
        }

        let m_fast = self.m_fast.unwrap_or(Decimal::ZERO);
        let am_fast = self.am_fast.unwrap_or(Decimal::ONE);
        if am_fast.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let tsi = m_fast / am_fast * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(tsi))
    }

    fn is_ready(&self) -> bool {
        self.m_fast.is_some() && self.fast_count >= self.fast
    }

    fn period(&self) -> usize {
        self.slow
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.m_slow = None;
        self.m_fast = None;
        self.am_slow = None;
        self.am_fast = None;
        self.slow_count = 0;
        self.slow_m_sum = Decimal::ZERO;
        self.slow_am_sum = Decimal::ZERO;
        self.fast_m_sum = None;
        self.fast_am_sum = None;
        self.fast_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_tsi_period_0_fails() {
        assert!(Tsi::new("tsi", 0, 13).is_err());
        assert!(Tsi::new("tsi", 25, 0).is_err());
    }

    #[test]
    fn test_tsi_unavailable_before_warmup() {
        let mut tsi = Tsi::new("tsi", 5, 3).unwrap();
        for _ in 0..6 {
            assert_eq!(tsi.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!tsi.is_ready());
    }

    #[test]
    fn test_tsi_reset() {
        let mut tsi = Tsi::new("tsi", 5, 3).unwrap();
        for i in 0..30 {
            tsi.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        tsi.reset();
        assert!(!tsi.is_ready());
    }

    #[test]
    fn test_tsi_uptrend_positive() {
        // Consistent uptrend: TSI should be positive
        let mut tsi = Tsi::new("tsi", 5, 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..40u32 {
            last = tsi.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > Decimal::ZERO, "TSI should be positive in uptrend, got {v}");
        } else {
            panic!("expected Scalar, got Unavailable");
        }
    }

    #[test]
    fn test_tsi_constant_price_is_zero() {
        // Flat price: all momentums are 0, TSI = 0/0 = 0
        let mut tsi = Tsi::new("tsi", 5, 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..40 {
            last = tsi.update_bar(&bar("100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }
}
