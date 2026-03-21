//! Open Range Position indicator.
//!
//! Measures where the open price sits within the bar's high-low range, normalized
//! to [0, 1]. Tracks the EMA of this per-bar value.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `(open − low) / (high − low)` per bar.
///
/// For each bar the raw value is:
/// ```text
/// raw = (open - low) / (high - low)   when high > low
///     = 0                              when high == low (flat bar)
/// ```
///
/// Ranges from `0.0` (opened at low) to `1.0` (opened at high). A persistent
/// high value indicates systematic gap-up opens (bullish gap pressure); a
/// persistent low value indicates gap-down opens (bearish gap pressure).
///
/// Returns a value after the first bar (EMA seeds with the first raw value).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenRangePosition;
/// use fin_primitives::signals::Signal;
///
/// let orp = OpenRangePosition::new("orp", 10).unwrap();
/// assert_eq!(orp.period(), 10);
/// ```
pub struct OpenRangePosition {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl OpenRangePosition {
    /// Constructs a new `OpenRangePosition`.
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

impl crate::signals::Signal for OpenRangePosition {
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
            (bar.open - bar.low)
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
    fn test_orp_invalid_period() {
        assert!(OpenRangePosition::new("orp", 0).is_err());
    }

    #[test]
    fn test_orp_ready_after_first_bar() {
        let mut orp = OpenRangePosition::new("orp", 5).unwrap();
        orp.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(orp.is_ready());
    }

    #[test]
    fn test_orp_open_at_low_zero() {
        let mut orp = OpenRangePosition::new("orp", 5).unwrap();
        // open=90=low: (90-90)/20 = 0
        let v = orp.update_bar(&bar("90", "110", "90", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_orp_open_at_high_one() {
        let mut orp = OpenRangePosition::new("orp", 5).unwrap();
        // open=110=high: (110-90)/20 = 1
        let v = orp.update_bar(&bar("110", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_orp_open_at_midpoint_half() {
        let mut orp = OpenRangePosition::new("orp", 5).unwrap();
        // open=100, range=20: (100-90)/20 = 0.5
        let v = orp.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_orp_flat_bar_zero() {
        let mut orp = OpenRangePosition::new("orp", 5).unwrap();
        let v = orp.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_orp_reset() {
        let mut orp = OpenRangePosition::new("orp", 5).unwrap();
        orp.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(orp.is_ready());
        orp.reset();
        assert!(!orp.is_ready());
    }
}
