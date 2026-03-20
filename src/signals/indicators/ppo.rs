//! Percentage Price Oscillator (PPO) indicator.

use crate::error::FinError;
use crate::signals::indicators::Ema;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Percentage Price Oscillator: `(EMA(fast) - EMA(slow)) / EMA(slow) * 100`.
///
/// Like the APO but expressed as a percentage of the slow EMA, making it
/// comparable across different price levels.
///
/// Returns `SignalValue::Unavailable` until the slow EMA is ready.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Ppo;
/// use fin_primitives::signals::Signal;
///
/// let mut ppo = Ppo::new("ppo_3_7", 3, 7).unwrap();
/// ```
pub struct Ppo {
    name: String,
    fast: Ema,
    slow: Ema,
    slow_period: usize,
}

impl Ppo {
    /// Constructs a new `Ppo` with the given name, fast period, and slow period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero or if
    /// `fast_period >= slow_period`.
    pub fn new(
        name: impl Into<String>,
        fast_period: usize,
        slow_period: usize,
    ) -> Result<Self, FinError> {
        if fast_period == 0 || slow_period == 0 {
            return Err(FinError::InvalidPeriod(fast_period.max(slow_period)));
        }
        if fast_period >= slow_period {
            return Err(FinError::InvalidPeriod(fast_period));
        }
        let name = name.into();
        Ok(Self {
            fast: Ema::new(format!("{name}_fast"), fast_period)?,
            slow: Ema::new(format!("{name}_slow"), slow_period)?,
            name,
            slow_period,
        })
    }
}

impl Signal for Ppo {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let fast_val = self.fast.update(bar)?;
        let slow_val = self.slow.update(bar)?;
        match (fast_val, slow_val) {
            (SignalValue::Scalar(f), SignalValue::Scalar(s)) => {
                if s.is_zero() {
                    return Ok(SignalValue::Unavailable);
                }
                Ok(SignalValue::Scalar((f - s) / s * Decimal::ONE_HUNDRED))
            }
            _ => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool {
        self.slow.is_ready()
    }

    fn period(&self) -> usize {
        self.slow_period
    }

    fn reset(&mut self) {
        self.fast.reset();
        self.slow.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_ppo_period_0_error() {
        assert!(Ppo::new("ppo", 0, 5).is_err());
        assert!(Ppo::new("ppo", 3, 0).is_err());
    }

    #[test]
    fn test_ppo_fast_ge_slow_error() {
        assert!(Ppo::new("ppo", 5, 3).is_err());
        assert!(Ppo::new("ppo", 5, 5).is_err());
    }

    #[test]
    fn test_ppo_unavailable_before_slow_ready() {
        let mut ppo = Ppo::new("ppo", 2, 4).unwrap();
        for p in &["100", "102", "104"] {
            assert_eq!(ppo.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!ppo.is_ready());
    }

    #[test]
    fn test_ppo_constant_prices_zero() {
        let mut ppo = Ppo::new("ppo", 2, 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = ppo.update_bar(&bar("100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ppo_ready_after_slow_period() {
        let mut ppo = Ppo::new("ppo", 2, 3).unwrap();
        for p in &["100", "102", "104", "106"] {
            ppo.update_bar(&bar(p)).unwrap();
        }
        assert!(ppo.is_ready());
        assert!(matches!(ppo.update_bar(&bar("108")).unwrap(), SignalValue::Scalar(_)));
    }

    #[test]
    fn test_ppo_period_returns_slow_period() {
        let ppo = Ppo::new("ppo", 3, 9).unwrap();
        assert_eq!(ppo.period(), 9);
    }

    #[test]
    fn test_ppo_reset_clears_state() {
        let mut ppo = Ppo::new("ppo", 2, 3).unwrap();
        for p in &["100", "102", "104", "106"] {
            ppo.update_bar(&bar(p)).unwrap();
        }
        assert!(ppo.is_ready());
        ppo.reset();
        assert!(!ppo.is_ready());
    }
}
