//! Absolute Price Oscillator (APO) indicator.

use crate::error::FinError;
use crate::signals::indicators::Ema;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Absolute Price Oscillator: `EMA(fast) - EMA(slow)`.
///
/// The APO measures the distance between two EMAs. Positive values indicate
/// upward momentum; negative values indicate downward momentum.
///
/// Returns `SignalValue::Unavailable` until the slow EMA has enough data
/// (i.e., until `slow_period` bars have been seen).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Apo;
/// use fin_primitives::signals::Signal;
///
/// let mut apo = Apo::new("apo_3_5", 3, 5).unwrap();
/// ```
pub struct Apo {
    name: String,
    fast: Ema,
    slow: Ema,
    slow_period: usize,
}

impl Apo {
    /// Constructs a new `Apo` with the given name, fast period, and slow period.
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

impl Signal for Apo {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let fast_val = self.fast.update(bar)?;
        let slow_val = self.slow.update(bar)?;
        match (fast_val, slow_val) {
            (SignalValue::Scalar(f), SignalValue::Scalar(s)) => {
                Ok(SignalValue::Scalar(f - s))
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
    fn test_apo_period_0_error() {
        assert!(Apo::new("apo", 0, 5).is_err());
        assert!(Apo::new("apo", 3, 0).is_err());
    }

    #[test]
    fn test_apo_fast_ge_slow_error() {
        assert!(Apo::new("apo", 5, 3).is_err());
        assert!(Apo::new("apo", 5, 5).is_err());
    }

    #[test]
    fn test_apo_unavailable_before_slow_ready() {
        let mut apo = Apo::new("apo", 2, 4).unwrap();
        for p in &["100", "102", "104"] {
            let v = apo.update_bar(&bar(p)).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
        assert!(!apo.is_ready());
    }

    #[test]
    fn test_apo_ready_after_slow_period() {
        let mut apo = Apo::new("apo", 2, 3).unwrap();
        for p in &["100", "102", "104"] {
            apo.update_bar(&bar(p)).unwrap();
        }
        assert!(apo.is_ready());
        let v = apo.update_bar(&bar("106")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_apo_constant_prices_zero() {
        // With constant prices both EMAs converge to the same value → APO = 0
        let mut apo = Apo::new("apo", 2, 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = apo.update_bar(&bar("100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_apo_rising_prices_positive() {
        // In a rising market fast EMA > slow EMA → APO > 0
        let mut apo = Apo::new("apo", 2, 4).unwrap();
        for i in 1..=10u32 {
            apo.update_bar(&bar(&i.to_string())).unwrap();
        }
        if let SignalValue::Scalar(v) = apo.update_bar(&bar("11")).unwrap() {
            assert!(v > dec!(0), "expected positive APO in uptrend, got {v}");
        }
    }

    #[test]
    fn test_apo_reset_clears_state() {
        let mut apo = Apo::new("apo", 2, 3).unwrap();
        for p in &["100", "102", "104", "106"] {
            apo.update_bar(&bar(p)).unwrap();
        }
        assert!(apo.is_ready());
        apo.reset();
        assert!(!apo.is_ready());
        let v = apo.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_apo_period_returns_slow_period() {
        let apo = Apo::new("apo", 3, 7).unwrap();
        assert_eq!(apo.period(), 7);
    }

    #[test]
    fn test_apo_name() {
        let apo = Apo::new("my_apo", 3, 7).unwrap();
        assert_eq!(apo.name(), "my_apo");
    }
}
