//! Close High-Low Spread indicator.
//!
//! Tracks the EMA of `(high - close) - (close - low)`, the net asymmetry
//! between the close's distance to the high versus its distance to the low.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `(high − close) − (close − low)`.
///
/// Equivalently: `high + low - 2 × close`, this measures whether the close
/// tends to lean toward the high (negative values = bullish) or the low
/// (positive values = bearish).
///
/// - **Positive**: close is closer to the low → bearish pressure.
/// - **Negative**: close is closer to the high → bullish pressure.
/// - **Zero**: close sits exactly at the bar midpoint.
///
/// Returns a value after the first bar (EMA seeds with the first raw value).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseHighLowSpread;
/// use fin_primitives::signals::Signal;
///
/// let chls = CloseHighLowSpread::new("chls", 10).unwrap();
/// assert_eq!(chls.period(), 10);
/// ```
pub struct CloseHighLowSpread {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl CloseHighLowSpread {
    /// Constructs a new `CloseHighLowSpread`.
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

impl crate::signals::Signal for CloseHighLowSpread {
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
        // (high - close) - (close - low) = high + low - 2*close
        let raw = bar.high + bar.low - Decimal::TWO * bar.close;

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
    fn test_chls_invalid_period() {
        assert!(CloseHighLowSpread::new("chls", 0).is_err());
    }

    #[test]
    fn test_chls_ready_after_first_bar() {
        let mut chls = CloseHighLowSpread::new("chls", 5).unwrap();
        chls.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(chls.is_ready());
    }

    #[test]
    fn test_chls_close_at_midpoint_zero() {
        let mut chls = CloseHighLowSpread::new("chls", 5).unwrap();
        // high=110, low=90, close=100: (110+90 - 200) = 0
        let v = chls.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_chls_close_at_high_negative() {
        let mut chls = CloseHighLowSpread::new("chls", 5).unwrap();
        // close=110 (at high): (110+90 - 220) = -20 → bullish
        let v = chls.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-20)));
    }

    #[test]
    fn test_chls_close_at_low_positive() {
        let mut chls = CloseHighLowSpread::new("chls", 5).unwrap();
        // close=90 (at low): (110+90 - 180) = 20 → bearish
        let v = chls.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_chls_reset() {
        let mut chls = CloseHighLowSpread::new("chls", 5).unwrap();
        chls.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(chls.is_ready());
        chls.reset();
        assert!(!chls.is_ready());
    }
}
