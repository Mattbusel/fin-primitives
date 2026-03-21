//! Low-to-Close Ratio indicator.
//!
//! Tracks the EMA of `low / close`, measuring how far the bar's low is from
//! the close on a relative basis — a smoothed measure of downside tail risk
//! relative to the closing price.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `low / close`.
///
/// Values near `1.0` indicate the low and close are very close together — the
/// bar has minimal downside tail (bullish structure). Lower values indicate the
/// low was far below the close — significant wicks below or price recovered
/// strongly from the low.
///
/// Returns `Unavailable` when `close` is zero. Returns a value after the first
/// bar (EMA seeds immediately).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LowToCloseRatio;
/// use fin_primitives::signals::Signal;
///
/// let ltcr = LowToCloseRatio::new("ltcr", 10).unwrap();
/// assert_eq!(ltcr.period(), 10);
/// ```
pub struct LowToCloseRatio {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl LowToCloseRatio {
    /// Constructs a new `LowToCloseRatio`.
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

impl crate::signals::Signal for LowToCloseRatio {
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
        if bar.close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = bar.low
            .checked_div(bar.close)
            .ok_or(FinError::ArithmeticOverflow)?;

        let ema = match self.ema {
            None => {
                self.ema = Some(ratio);
                ratio
            }
            Some(prev) => {
                let next = ratio * self.k + prev * (Decimal::ONE - self.k);
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

    fn bar(low: &str, close: &str) -> OhlcvBar {
        let l = Price::new(low.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let high = if c > l { c } else { l };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l, high, low: l, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ltcr_invalid_period() {
        assert!(LowToCloseRatio::new("ltcr", 0).is_err());
    }

    #[test]
    fn test_ltcr_ready_after_first_bar() {
        let mut ltcr = LowToCloseRatio::new("ltcr", 5).unwrap();
        ltcr.update_bar(&bar("90", "105")).unwrap();
        assert!(ltcr.is_ready());
    }

    #[test]
    fn test_ltcr_low_equals_close_one() {
        let mut ltcr = LowToCloseRatio::new("ltcr", 5).unwrap();
        // low==close: ratio = 1.0
        let v = ltcr.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ltcr_low_below_close() {
        let mut ltcr = LowToCloseRatio::new("ltcr", 5).unwrap();
        // low=90, close=100: ratio = 0.9
        let v = ltcr.update_bar(&bar("90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.9)));
    }

    #[test]
    fn test_ltcr_reset() {
        let mut ltcr = LowToCloseRatio::new("ltcr", 5).unwrap();
        ltcr.update_bar(&bar("90", "105")).unwrap();
        assert!(ltcr.is_ready());
        ltcr.reset();
        assert!(!ltcr.is_ready());
    }
}
