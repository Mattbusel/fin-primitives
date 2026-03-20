//! Envelope indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Envelope — SMA with upper and lower percentage bands.
///
/// ```text
/// Middle = SMA(period, close)
/// Upper  = Middle × (1 + pct/100)
/// Lower  = Middle × (1 - pct/100)
/// ```
///
/// Returns the **middle SMA** as the scalar value.
/// Use [`Envelope::upper`] and [`Envelope::lower`] for the band levels.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Envelope;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
///
/// let e = Envelope::new("env20", 20, dec!(2.5)).unwrap();
/// assert_eq!(e.period(), 20);
/// assert!(!e.is_ready());
/// ```
pub struct Envelope {
    name: String,
    period: usize,
    pct: Decimal,
    closes: VecDeque<Decimal>,
    upper: Option<Decimal>,
    lower: Option<Decimal>,
}

impl Envelope {
    /// Constructs a new `Envelope`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if `pct <= 0`.
    pub fn new(name: impl Into<String>, period: usize, pct: Decimal) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        if pct <= Decimal::ZERO {
            return Err(FinError::InvalidInput(format!("pct must be > 0, got {pct}")));
        }
        Ok(Self {
            name: name.into(),
            period,
            pct,
            closes: VecDeque::with_capacity(period),
            upper: None,
            lower: None,
        })
    }

    /// Returns the current upper band, or `None` if not ready.
    pub fn upper(&self) -> Option<Decimal> {
        self.upper
    }

    /// Returns the current lower band, or `None` if not ready.
    pub fn lower(&self) -> Option<Decimal> {
        self.lower
    }

    /// Returns the band width as `upper - lower`, or `None` if not ready.
    pub fn band_width(&self) -> Option<Decimal> {
        Some(self.upper? - self.lower?)
    }
}

impl Signal for Envelope {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        #[allow(clippy::cast_possible_truncation)]
        let sma = self.closes.iter().copied().sum::<Decimal>()
            / Decimal::from(self.period as u32);
        let factor = self.pct / Decimal::ONE_HUNDRED;
        self.upper = Some(sma * (Decimal::ONE + factor));
        self.lower = Some(sma * (Decimal::ONE - factor));
        Ok(SignalValue::Scalar(sma))
    }

    fn is_ready(&self) -> bool {
        self.upper.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.upper = None;
        self.lower = None;
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
    fn test_env_invalid_period() {
        assert!(Envelope::new("e", 0, dec!(2)).is_err());
    }

    #[test]
    fn test_env_invalid_pct() {
        assert!(Envelope::new("e", 10, dec!(0)).is_err());
        assert!(Envelope::new("e", 10, dec!(-1)).is_err());
    }

    #[test]
    fn test_env_unavailable_before_period() {
        let mut e = Envelope::new("e", 3, dec!(2)).unwrap();
        assert_eq!(e.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!e.is_ready());
    }

    #[test]
    fn test_env_bands_correct() {
        let mut e = Envelope::new("e", 3, dec!(10)).unwrap();
        for _ in 0..3 { e.update_bar(&bar("100")).unwrap(); }
        assert_eq!(e.upper(), Some(dec!(110)));
        assert_eq!(e.lower(), Some(dec!(90)));
        assert_eq!(e.band_width(), Some(dec!(20)));
    }

    #[test]
    fn test_env_scalar_is_sma() {
        let mut e = Envelope::new("e", 3, dec!(5)).unwrap();
        for _ in 0..3 { e.update_bar(&bar("100")).unwrap(); }
        let v = e.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_env_reset() {
        let mut e = Envelope::new("e", 3, dec!(2)).unwrap();
        for _ in 0..3 { e.update_bar(&bar("100")).unwrap(); }
        assert!(e.is_ready());
        e.reset();
        assert!(!e.is_ready());
        assert!(e.upper().is_none());
        assert!(e.lower().is_none());
    }

    #[test]
    fn test_env_period_and_name() {
        let e = Envelope::new("my_env", 20, dec!(2.5)).unwrap();
        assert_eq!(e.period(), 20);
        assert_eq!(e.name(), "my_env");
    }
}
