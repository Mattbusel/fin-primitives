//! EMA Alignment indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA Alignment — measures whether three EMAs (fast, medium, slow) are in
/// a fully aligned order, indicating a strong trend.
///
/// - `+1` → fast > medium > slow (bullish alignment)
/// - `-1` → fast < medium < slow (bearish alignment)
/// - `0` → mixed order (no clear trend)
///
/// Returns [`SignalValue::Scalar`] from the first bar (always ready, all EMAs
/// initialize to the first close).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EmaAlignment;
/// use fin_primitives::signals::Signal;
///
/// let ea = EmaAlignment::new("ea", 5, 13, 34).unwrap();
/// assert_eq!(ea.period(), 34);
/// ```
pub struct EmaAlignment {
    name: String,
    fast_k: Decimal,
    medium_k: Decimal,
    slow_k: Decimal,
    slow_period: usize,
    fast_ema: Option<Decimal>,
    medium_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
}

impl EmaAlignment {
    /// Constructs a new `EmaAlignment`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if periods are zero or not strictly ordered.
    pub fn new(
        name: impl Into<String>,
        fast: usize,
        medium: usize,
        slow: usize,
    ) -> Result<Self, FinError> {
        if fast == 0 || fast >= medium || medium >= slow {
            return Err(FinError::InvalidPeriod(fast));
        }
        let k = |p: usize| Decimal::TWO / (Decimal::from(p as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            fast_k: k(fast),
            medium_k: k(medium),
            slow_k: k(slow),
            slow_period: slow,
            fast_ema: None,
            medium_ema: None,
            slow_ema: None,
        })
    }

    fn ema_step(prev: Option<Decimal>, input: Decimal, k: Decimal) -> Decimal {
        match prev {
            None => input,
            Some(p) => k * input + (Decimal::ONE - k) * p,
        }
    }
}

impl Signal for EmaAlignment {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.slow_period }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let price = bar.close;
        let fast   = Self::ema_step(self.fast_ema,   price, self.fast_k);
        let medium = Self::ema_step(self.medium_ema, price, self.medium_k);
        let slow   = Self::ema_step(self.slow_ema,   price, self.slow_k);
        self.fast_ema   = Some(fast);
        self.medium_ema = Some(medium);
        self.slow_ema   = Some(slow);

        let signal = if fast > medium && medium > slow {
            Decimal::ONE
        } else if fast < medium && medium < slow {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::ZERO
        };
        Ok(SignalValue::Scalar(signal))
    }

    fn reset(&mut self) {
        self.fast_ema   = None;
        self.medium_ema = None;
        self.slow_ema   = None;
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
    fn test_ea_invalid() {
        assert!(EmaAlignment::new("e", 0, 5, 10).is_err());
        assert!(EmaAlignment::new("e", 10, 5, 20).is_err()); // fast >= medium
        assert!(EmaAlignment::new("e", 5, 10, 10).is_err()); // medium >= slow
    }

    #[test]
    fn test_ea_always_ready() {
        let ea = EmaAlignment::new("e", 3, 7, 14).unwrap();
        assert!(ea.is_ready());
    }

    #[test]
    fn test_ea_bullish_uptrend() {
        let mut ea = EmaAlignment::new("e", 3, 7, 14).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..30 {
            last = ea.update_bar(&bar(&(100 + i * 3).to_string())).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ea_bearish_downtrend() {
        let mut ea = EmaAlignment::new("e", 3, 7, 14).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..30 {
            last = ea.update_bar(&bar(&(300 - i * 3).to_string())).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_ea_reset() {
        let mut ea = EmaAlignment::new("e", 3, 7, 14).unwrap();
        for i in 0u32..30 { ea.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        ea.reset();
        assert!(ea.fast_ema.is_none());
    }
}
