//! Close Location EMA indicator.
//!
//! EMA of the close location value — where the close sits within the bar's
//! high-low range, on a -1 to +1 scale.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close Location EMA — smoothed close location value (CLV).
///
/// The raw CLV for each bar is:
/// ```text
/// CLV[i] = (2 × close - high - low) / (high - low)   when high > low
///        = 0                                          when high == low
/// ```
///
/// This ranges from `-1` (close at the low) to `+1` (close at the high),
/// with `0` meaning the close is exactly at the midpoint.
///
/// The EMA over `period` bars smooths out bar-to-bar noise. High positive
/// values indicate persistent closing near the top — buying pressure. Low
/// negative values indicate persistent closing near the bottom — selling
/// pressure.
///
/// Identical to the CLV used in the Accumulation/Distribution Line, but
/// exposed as a standalone smoothed signal.
///
/// Returns a value from the first bar (EMA seeds with first CLV).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseLocationEma;
/// use fin_primitives::signals::Signal;
/// let cle = CloseLocationEma::new("cle_14", 14).unwrap();
/// assert_eq!(cle.period(), 14);
/// ```
pub struct CloseLocationEma {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl CloseLocationEma {
    /// Constructs a new `CloseLocationEma`.
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

impl Signal for CloseLocationEma {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ema.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let clv = if range.is_zero() {
            Decimal::ZERO
        } else {
            // (2*close - high - low) / range
            let numerator = Decimal::from(2u32) * bar.close - bar.high - bar.low;
            numerator
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        let ema = match self.ema {
            None => {
                self.ema = Some(clv);
                clv
            }
            Some(prev) => {
                let next = clv * self.k + prev * (Decimal::ONE - self.k);
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cle_invalid_period() {
        assert!(CloseLocationEma::new("cle", 0).is_err());
    }

    #[test]
    fn test_cle_ready_after_first_bar() {
        let mut cle = CloseLocationEma::new("cle", 5).unwrap();
        cle.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(cle.is_ready());
    }

    #[test]
    fn test_cle_close_at_high_plus_one() {
        // CLV = (2*110 - 110 - 90) / 20 = 20/20 = 1
        let mut cle = CloseLocationEma::new("cle", 5).unwrap();
        let v = cle.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cle_close_at_low_minus_one() {
        // CLV = (2*90 - 110 - 90) / 20 = -20/20 = -1
        let mut cle = CloseLocationEma::new("cle", 5).unwrap();
        let v = cle.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_cle_close_at_midpoint_zero() {
        // CLV = (2*100 - 110 - 90) / 20 = 0
        let mut cle = CloseLocationEma::new("cle", 5).unwrap();
        let v = cle.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cle_flat_bar_zero() {
        let mut cle = CloseLocationEma::new("cle", 5).unwrap();
        let v = cle.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cle_persistent_high_closes_positive() {
        let mut cle = CloseLocationEma::new("cle", 3).unwrap();
        for _ in 0..10 {
            cle.update_bar(&bar("110", "90", "108")).unwrap(); // CLV = (216-110-90)/20 = 0.8
        }
        if let SignalValue::Scalar(v) = cle.update_bar(&bar("110", "90", "108")).unwrap() {
            assert!(v > dec!(0), "persistent high closes → positive EMA CLV: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cle_reset() {
        let mut cle = CloseLocationEma::new("cle", 5).unwrap();
        cle.update_bar(&bar("110", "90", "105")).unwrap();
        assert!(cle.is_ready());
        cle.reset();
        assert!(!cle.is_ready());
    }
}
