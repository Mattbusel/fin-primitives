//! Body-to-Range EMA indicator.
//!
//! Tracks the EMA of each bar's body-to-range ratio, measuring how much of the
//! bar's range is captured by the open-to-close body on a smoothed basis.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `body_size / range` per bar.
///
/// For each bar the raw ratio is:
/// ```text
/// raw = |close - open| / (high - low)   when high > low
///     = 0                                when high == low (flat bar)
/// ```
///
/// This ranges from `0.0` (doji / no body) to `1.0` (body fills the entire range).
/// The EMA smooths the ratio over `period` bars. High values indicate bars with
/// strong directional conviction; low values indicate indecision or wicks.
///
/// Returns a value after the first bar (EMA seeds with the first raw value).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyToRangeEma;
/// use fin_primitives::signals::Signal;
///
/// let btr = BodyToRangeEma::new("btr", 10).unwrap();
/// assert_eq!(btr.period(), 10);
/// ```
pub struct BodyToRangeEma {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl BodyToRangeEma {
    /// Constructs a new `BodyToRangeEma`.
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

impl Signal for BodyToRangeEma {
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
            bar.body_size()
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
    fn test_btr_invalid_period() {
        assert!(BodyToRangeEma::new("btr", 0).is_err());
    }

    #[test]
    fn test_btr_ready_after_first_bar() {
        let mut btr = BodyToRangeEma::new("btr", 5).unwrap();
        btr.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(btr.is_ready());
    }

    #[test]
    fn test_btr_full_body_returns_one() {
        let mut btr = BodyToRangeEma::new("btr", 5).unwrap();
        // open=90, high=110, low=90, close=110 → body=20, range=20 → ratio=1.0
        let v = btr.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_btr_doji_returns_zero() {
        let mut btr = BodyToRangeEma::new("btr", 5).unwrap();
        // open==close: body=0
        let v = btr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_btr_flat_bar_returns_zero() {
        let mut btr = BodyToRangeEma::new("btr", 5).unwrap();
        let v = btr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_btr_ema_smoothing() {
        let mut btr = BodyToRangeEma::new("btr", 3).unwrap();
        // seed with full body bar
        btr.update_bar(&bar("90", "110", "90", "110")).unwrap();
        // then doji — EMA should decay toward 0 but stay positive
        let v = btr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        if let SignalValue::Scalar(e) = v {
            assert!(e > dec!(0) && e < dec!(1), "EMA should be between 0 and 1: {e}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_btr_reset() {
        let mut btr = BodyToRangeEma::new("btr", 5).unwrap();
        btr.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(btr.is_ready());
        btr.reset();
        assert!(!btr.is_ready());
    }
}
