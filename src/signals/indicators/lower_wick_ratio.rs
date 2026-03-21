//! Lower Wick Ratio indicator.
//!
//! Tracks the EMA of each bar's lower wick as a fraction of the bar's total
//! range, providing a smoothed measure of downside rejection / support strength.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `lower_wick / range` per bar.
///
/// For each bar the raw ratio is:
/// ```text
/// raw = (min(open, close) - low) / (high - low)   when high > low
///     = 0                                          when high == low (flat bar)
/// ```
///
/// Values near `1.0` indicate a long lower shadow with open and close near the
/// high — strong support / buying into weakness. Values near `0.0` indicate
/// open/close near the low — no downside rejection.
///
/// Returns a value after the first bar (EMA seeds with the first raw value).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LowerWickRatio;
/// use fin_primitives::signals::Signal;
///
/// let lwr = LowerWickRatio::new("lwr", 10).unwrap();
/// assert_eq!(lwr.period(), 10);
/// ```
pub struct LowerWickRatio {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl LowerWickRatio {
    /// Constructs a new `LowerWickRatio`.
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

impl Signal for LowerWickRatio {
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
            bar.lower_wick()
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
    fn test_lwr_invalid_period() {
        assert!(LowerWickRatio::new("lwr", 0).is_err());
    }

    #[test]
    fn test_lwr_ready_after_first_bar() {
        let mut lwr = LowerWickRatio::new("lwr", 5).unwrap();
        lwr.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(lwr.is_ready());
    }

    #[test]
    fn test_lwr_no_lower_wick_returns_zero() {
        let mut lwr = LowerWickRatio::new("lwr", 5).unwrap();
        // open==low: no lower wick
        let v = lwr.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_lwr_full_lower_wick() {
        let mut lwr = LowerWickRatio::new("lwr", 5).unwrap();
        // open==close==high: entire range is lower wick → ratio = 1.0
        let v = lwr.update_bar(&bar("110", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_lwr_flat_bar_zero() {
        let mut lwr = LowerWickRatio::new("lwr", 5).unwrap();
        let v = lwr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_lwr_persistent_lower_wicks_positive() {
        let mut lwr = LowerWickRatio::new("lwr", 3).unwrap();
        for _ in 0..5 {
            // hammer-like bars: open==close==high
            lwr.update_bar(&bar("110", "110", "90", "110")).unwrap();
        }
        let v = lwr.update_bar(&bar("110", "110", "90", "110")).unwrap();
        if let SignalValue::Scalar(e) = v {
            assert!(e > dec!(0.9), "persistent lower wicks → EMA near 1: {e}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_lwr_reset() {
        let mut lwr = LowerWickRatio::new("lwr", 5).unwrap();
        lwr.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(lwr.is_ready());
        lwr.reset();
        assert!(!lwr.is_ready());
    }
}
