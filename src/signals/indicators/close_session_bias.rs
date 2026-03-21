//! Close Session Bias indicator.
//!
//! Tracks the EMA of `(close - open) / typical_price`, providing a
//! price-normalized measure of intrabar directional bias per session.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `(close − open) / typical_price`.
///
/// The typical price `(high + low + close) / 3` is used as the normalizer,
/// making the raw ratio scale-invariant across price levels. Each bar's raw
/// contribution is:
/// ```text
/// raw = (close - open) / ((high + low + close) / 3)
///     = 0   when typical_price == 0
/// ```
///
/// Positive values indicate persistent bullish intrabar momentum (close above
/// open) relative to the price scale. Negative values indicate persistent
/// bearish intrabar momentum.
///
/// Returns a value after the first bar (EMA seeds immediately).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseSessionBias;
/// use fin_primitives::signals::Signal;
///
/// let csb = CloseSessionBias::new("csb", 10).unwrap();
/// assert_eq!(csb.period(), 10);
/// ```
pub struct CloseSessionBias {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl CloseSessionBias {
    /// Constructs a new `CloseSessionBias`.
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

impl crate::signals::Signal for CloseSessionBias {
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
        let tp = bar.typical_price();
        let raw = if tp.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - bar.open)
                .checked_div(tp)
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
    fn test_csb_invalid_period() {
        assert!(CloseSessionBias::new("csb", 0).is_err());
    }

    #[test]
    fn test_csb_ready_after_first_bar() {
        let mut csb = CloseSessionBias::new("csb", 5).unwrap();
        csb.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(csb.is_ready());
    }

    #[test]
    fn test_csb_neutral_bar_zero() {
        let mut csb = CloseSessionBias::new("csb", 5).unwrap();
        // open == close → raw = 0
        let v = csb.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_csb_bullish_positive() {
        let mut csb = CloseSessionBias::new("csb", 5).unwrap();
        // open=90, close=110, tp=(110+90+110)/3=103.333... → raw = 20/103.333 > 0
        let v = csb.update_bar(&bar("90", "110", "90", "110")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0), "bullish bar → positive CSB: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_csb_bearish_negative() {
        let mut csb = CloseSessionBias::new("csb", 5).unwrap();
        // open=110, close=90 → raw < 0
        let v = csb.update_bar(&bar("110", "110", "90", "90")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s < dec!(0), "bearish bar → negative CSB: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_csb_reset() {
        let mut csb = CloseSessionBias::new("csb", 5).unwrap();
        csb.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(csb.is_ready());
        csb.reset();
        assert!(!csb.is_ready());
    }
}
