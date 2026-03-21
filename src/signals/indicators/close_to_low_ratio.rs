//! Close-to-Low Ratio indicator.
//!
//! Tracks the EMA of `(close - low) / (high - low)`, measuring where the close
//! sits within the bar range as a fraction. This is the positive half of the
//! close location value, normalized to [0, 1].

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `(close − low) / (high − low)`.
///
/// For each bar:
/// ```text
/// raw = (close - low) / (high - low)   when high > low
///     = 0                               when high == low (flat bar)
/// ```
///
/// Ranges from `0.0` (close at low) to `1.0` (close at high). The EMA smooths
/// this over `period` bars. Persistent values near `1.0` indicate strong bullish
/// closing behavior (closes near highs); near `0.0` indicates closes near lows.
///
/// Returns a value after the first bar (EMA seeds immediately).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseToLowRatio;
/// use fin_primitives::signals::Signal;
///
/// let ctlr = CloseToLowRatio::new("ctlr", 10).unwrap();
/// assert_eq!(ctlr.period(), 10);
/// ```
pub struct CloseToLowRatio {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl CloseToLowRatio {
    /// Constructs a new `CloseToLowRatio`.
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

impl crate::signals::Signal for CloseToLowRatio {
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
            (bar.close - bar.low)
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
    fn test_ctlr_invalid_period() {
        assert!(CloseToLowRatio::new("ctlr", 0).is_err());
    }

    #[test]
    fn test_ctlr_ready_after_first_bar() {
        let mut ctlr = CloseToLowRatio::new("ctlr", 5).unwrap();
        ctlr.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(ctlr.is_ready());
    }

    #[test]
    fn test_ctlr_close_at_low_zero() {
        let mut ctlr = CloseToLowRatio::new("ctlr", 5).unwrap();
        let v = ctlr.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ctlr_close_at_high_one() {
        let mut ctlr = CloseToLowRatio::new("ctlr", 5).unwrap();
        let v = ctlr.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ctlr_close_at_midpoint_half() {
        let mut ctlr = CloseToLowRatio::new("ctlr", 5).unwrap();
        // close=100, range=20: (100-90)/20 = 0.5
        let v = ctlr.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_ctlr_flat_bar_zero() {
        let mut ctlr = CloseToLowRatio::new("ctlr", 5).unwrap();
        let v = ctlr.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ctlr_reset() {
        let mut ctlr = CloseToLowRatio::new("ctlr", 5).unwrap();
        ctlr.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(ctlr.is_ready());
        ctlr.reset();
        assert!(!ctlr.is_ready());
    }
}
