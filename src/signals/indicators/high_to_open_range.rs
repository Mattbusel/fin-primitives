//! High-to-Open Range indicator.
//!
//! Tracks the EMA of `(high - open) / range`, measuring what fraction of the
//! bar's range lies above the open — the relative upside exploration from the
//! opening price.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `(high − open) / (high − low)`.
///
/// For each bar:
/// ```text
/// raw = (high - open) / (high - low)   when high > low
///     = 0                               when high == low (flat bar)
/// ```
///
/// Ranges from `0.0` (open at high, no upside from open) to `1.0` (open at
/// low, the entire range is upside from the open). High values indicate the
/// bar frequently rallies well above the open.
///
/// Returns a value after the first bar (EMA seeds immediately).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighToOpenRange;
/// use fin_primitives::signals::Signal;
///
/// let htor = HighToOpenRange::new("htor", 10).unwrap();
/// assert_eq!(htor.period(), 10);
/// ```
pub struct HighToOpenRange {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl HighToOpenRange {
    /// Constructs a new `HighToOpenRange`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::from(2u32) / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { name: name.into(), period, ema: None, k })
    }
}

impl crate::signals::Signal for HighToOpenRange {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.ema.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let raw = if range.is_zero() {
            Decimal::ZERO
        } else {
            (bar.high - bar.open)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        let ema = match self.ema {
            None => {
                self.ema = Some(raw);
                raw
            }
            Some(prev) => {
                let next = raw * self.k + prev * (Decimal::ONE - self.k);
                self.ema = Some(next);
                next
            }
        };

        Ok(SignalValue::Scalar(ema))
    }

    fn reset(&mut self) {
        self.ema = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, high: &str, low: &str, close: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(open.parse().unwrap()).unwrap(),
            high: Price::new(high.parse().unwrap()).unwrap(),
            low: Price::new(low.parse().unwrap()).unwrap(),
            close: Price::new(close.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_htor_invalid_period() {
        assert!(HighToOpenRange::new("htor", 0).is_err());
    }

    #[test]
    fn test_htor_ready_after_first_bar() {
        let mut htor = HighToOpenRange::new("htor", 5).unwrap();
        htor.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(htor.is_ready());
    }

    #[test]
    fn test_htor_open_at_high_zero() {
        let mut htor = HighToOpenRange::new("htor", 5).unwrap();
        // open=110=high: (110-110)/20 = 0
        let v = htor.update_bar(&bar("110", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_htor_open_at_low_one() {
        let mut htor = HighToOpenRange::new("htor", 5).unwrap();
        // open=90=low, high=110: (110-90)/20 = 1
        let v = htor.update_bar(&bar("90", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_htor_flat_bar_zero() {
        let mut htor = HighToOpenRange::new("htor", 5).unwrap();
        let v = htor.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_htor_reset() {
        let mut htor = HighToOpenRange::new("htor", 5).unwrap();
        htor.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(htor.is_ready());
        htor.reset();
        assert!(!htor.is_ready());
    }
}
