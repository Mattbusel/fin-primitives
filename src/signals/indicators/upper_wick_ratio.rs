//! Upper Wick Ratio indicator.
//!
//! Tracks the EMA of each bar's upper wick as a fraction of the bar's total
//! range, providing a smoothed measure of overhead rejection pressure.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `upper_wick / range` per bar.
///
/// For each bar the raw ratio is:
/// ```text
/// raw = (high - max(open, close)) / (high - low)   when high > low
///     = 0                                           when high == low (flat bar)
/// ```
///
/// Values near `1.0` indicate a long upper shadow with a very small body near
/// the low — strong overhead rejection. Values near `0.0` indicate close/open
/// near the high — no overhead rejection.
///
/// Returns a value after the first bar (EMA seeds with the first raw value).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpperWickRatio;
/// use fin_primitives::signals::Signal;
///
/// let uwr = UpperWickRatio::new("uwr", 10).unwrap();
/// assert_eq!(uwr.period(), 10);
/// ```
pub struct UpperWickRatio {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl UpperWickRatio {
    /// Constructs a new `UpperWickRatio`.
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

impl Signal for UpperWickRatio {
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
            bar.upper_wick()
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
    fn test_uwr_invalid_period() {
        assert!(UpperWickRatio::new("uwr", 0).is_err());
    }

    #[test]
    fn test_uwr_ready_after_first_bar() {
        let mut uwr = UpperWickRatio::new("uwr", 5).unwrap();
        uwr.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(uwr.is_ready());
    }

    #[test]
    fn test_uwr_no_upper_wick_returns_zero() {
        let mut uwr = UpperWickRatio::new("uwr", 5).unwrap();
        // close==high: no upper wick
        let v = uwr.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_uwr_full_upper_wick() {
        let mut uwr = UpperWickRatio::new("uwr", 5).unwrap();
        // open==close==low: entire range is upper wick → ratio = 1.0
        let v = uwr.update_bar(&bar("90", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_uwr_flat_bar_zero() {
        let mut uwr = UpperWickRatio::new("uwr", 5).unwrap();
        let v = uwr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_uwr_reset() {
        let mut uwr = UpperWickRatio::new("uwr", 5).unwrap();
        uwr.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(uwr.is_ready());
        uwr.reset();
        assert!(!uwr.is_ready());
    }
}
