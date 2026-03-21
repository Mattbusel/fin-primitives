//! High-Low Symmetry indicator.
//!
//! Tracks the EMA of `|(high - mid) - (mid - low)| / range`, measuring how
//! asymmetric each bar is around its midpoint. A perfectly symmetric bar has
//! equal distance from high to mid and from mid to low.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `|(high − mid) − (mid − low)| / range`.
///
/// For each bar:
/// ```text
/// mid        = (high + low) / 2
/// upper_half = high - mid   = (high - low) / 2
/// lower_half = mid - low    = (high - low) / 2
/// symmetry   = |upper_half - lower_half| / range = 0   (always symmetric!)
/// ```
///
/// Wait — (high - mid) == (mid - low) always. Instead we use **open** as the
/// reference rather than mid:
/// ```text
/// raw = |(high - open) - (open - low)| / range   when range > 0
///     = 0                                          when range == 0
/// ```
///
/// This measures how asymmetrically the open splits the bar's range.
/// A value of `0` means the open sits exactly at the midpoint.
/// A value of `1` means the open is at one extreme (high or low).
///
/// Returns a value after the first bar (EMA seeds immediately).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowSymmetry;
/// use fin_primitives::signals::Signal;
///
/// let hls = HighLowSymmetry::new("hls", 10).unwrap();
/// assert_eq!(hls.period(), 10);
/// ```
pub struct HighLowSymmetry {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl HighLowSymmetry {
    /// Constructs a new `HighLowSymmetry`.
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

impl crate::signals::Signal for HighLowSymmetry {
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
            let upper = bar.high - bar.open;
            let lower = bar.open - bar.low;
            (upper - lower)
                .abs()
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
    fn test_hls_invalid_period() {
        assert!(HighLowSymmetry::new("hls", 0).is_err());
    }

    #[test]
    fn test_hls_ready_after_first_bar() {
        let mut hls = HighLowSymmetry::new("hls", 5).unwrap();
        hls.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(hls.is_ready());
    }

    #[test]
    fn test_hls_symmetric_open_zero() {
        let mut hls = HighLowSymmetry::new("hls", 5).unwrap();
        // open=100, high=110, low=90: upper=10, lower=10 → |0|/20 = 0
        let v = hls.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hls_open_at_high_one() {
        let mut hls = HighLowSymmetry::new("hls", 5).unwrap();
        // open=110=high, low=90: upper=0, lower=20 → |0-20|/20 = 1
        let v = hls.update_bar(&bar("110", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_hls_flat_bar_zero() {
        let mut hls = HighLowSymmetry::new("hls", 5).unwrap();
        let v = hls.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hls_in_range() {
        let mut hls = HighLowSymmetry::new("hls", 3).unwrap();
        for _ in 0..5 {
            hls.update_bar(&bar("95", "110", "90", "105")).unwrap();
        }
        let v = hls.update_bar(&bar("95", "110", "90", "105")).unwrap();
        if let SignalValue::Scalar(e) = v {
            assert!(e >= dec!(0) && e <= dec!(1), "symmetry out of [0,1]: {e}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hls_reset() {
        let mut hls = HighLowSymmetry::new("hls", 5).unwrap();
        hls.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(hls.is_ready());
        hls.reset();
        assert!(!hls.is_ready());
    }
}
