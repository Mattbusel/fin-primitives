//! Close Range Position indicator.
//!
//! Tracks the EMA of the close's fractional position within each bar's range,
//! providing a smoothed measure of whether closes consistently sit near the
//! high or low of the bar.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA-smoothed close range position: EMA of `(close - low) / (high - low)`.
///
/// The raw position is `(close - low) / range`:
/// - `1.0`: close at the bar's high (strong bullish close).
/// - `0.0`: close at the bar's low (strong bearish close).
/// - `0.5`: close at midrange.
///
/// The EMA smooths this across bars, revealing whether recent closes
/// persistently favour the upper or lower portion of their ranges.
///
/// Flat-range bars (where `high == low`) contribute `0.5` to the EMA.
///
/// Returns [`SignalValue::Unavailable`] until the EMA has warmed up
/// (first bar always returns the raw position as the initial EMA seed).
/// After the first bar `is_ready()` returns `true`.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseRangePosition;
/// use fin_primitives::signals::Signal;
///
/// let crp = CloseRangePosition::new("crp", 10).unwrap();
/// assert_eq!(crp.period(), 10);
/// assert!(!crp.is_ready());
/// ```
pub struct CloseRangePosition {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl CloseRangePosition {
    /// Constructs a new `CloseRangePosition`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::from(2u32)
            / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { name: name.into(), period, ema: None, k })
    }

    fn raw_position(bar: &BarInput) -> Decimal {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Decimal::new(5, 1); // 0.5
        }
        (bar.close - bar.low) / range
    }
}

impl Signal for CloseRangePosition {
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
        let pos = Self::raw_position(bar);

        let ema = match self.ema {
            None => {
                self.ema = Some(pos);
                pos
            }
            Some(prev) => {
                let next = pos * self.k + prev * (Decimal::ONE - self.k);
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

    fn bar(high: &str, low: &str, close: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(low.parse().unwrap()).unwrap(),
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
    fn test_crp_invalid_period() {
        assert!(CloseRangePosition::new("crp", 0).is_err());
    }

    #[test]
    fn test_crp_ready_after_first_bar() {
        let mut crp = CloseRangePosition::new("crp", 5).unwrap();
        crp.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(crp.is_ready());
    }

    #[test]
    fn test_crp_close_at_high_seeds_one() {
        let mut crp = CloseRangePosition::new("crp", 5).unwrap();
        let v = crp.update_bar(&bar("110", "90", "110")).unwrap();
        // pos = (110 - 90) / (110 - 90) = 1; first bar seeds EMA = 1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_crp_close_at_low_seeds_zero() {
        let mut crp = CloseRangePosition::new("crp", 5).unwrap();
        let v = crp.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_crp_flat_bar_seeds_half() {
        let mut crp = CloseRangePosition::new("crp", 5).unwrap();
        let v = crp.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_crp_ema_decays_toward_new_value() {
        let mut crp = CloseRangePosition::new("crp", 2).unwrap();
        crp.update_bar(&bar("110", "90", "110")).unwrap(); // seed = 1
        // k = 2/(2+1) = 2/3; pos = 0; ema = 0 * 2/3 + 1 * 1/3 = 1/3
        let v = crp.update_bar(&bar("110", "90", "90")).unwrap();
        if let SignalValue::Scalar(e) = v {
            assert!(e > dec!(0) && e < dec!(1), "EMA should be between 0 and 1: {e}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_crp_reset() {
        let mut crp = CloseRangePosition::new("crp", 5).unwrap();
        crp.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(crp.is_ready());
        crp.reset();
        assert!(!crp.is_ready());
    }
}
