//! Close-to-VWAP Percentage Distance indicator.
//!
//! Measures how far the closing price is from the session VWAP, expressed as
//! a percentage of VWAP.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close-to-VWAP Percentage Distance: `(close - vwap) / vwap * 100`.
///
/// Tracks the rolling cumulative VWAP (sum of typical_price × volume / sum of
/// volume) and expresses the current close's deviation from it as a percentage.
/// Positive values mean the close is above VWAP; negative values mean below.
///
/// The rolling VWAP resets each time `reset()` is called. Returns
/// [`SignalValue::Unavailable`] when cumulative volume is zero (e.g. the first
/// bar has zero volume).
///
/// Ready after the first bar with non-zero volume.
///
/// # Errors
/// Returns [`FinError::ArithmeticOverflow`] on arithmetic failure.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseToVwapPct;
/// use fin_primitives::signals::Signal;
///
/// let cv = CloseToVwapPct::new("cv_pct").unwrap();
/// assert_eq!(cv.period(), 1);
/// assert!(!cv.is_ready());
/// ```
pub struct CloseToVwapPct {
    name: String,
    cum_tp_vol: Decimal,
    cum_vol: Decimal,
    ready: bool,
}

impl CloseToVwapPct {
    /// Constructs a new `CloseToVwapPct`.
    ///
    /// # Errors
    /// Never fails; returns `Ok` always.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            cum_tp_vol: Decimal::ZERO,
            cum_vol: Decimal::ZERO,
            ready: false,
        })
    }
}

impl Signal for CloseToVwapPct {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tp = bar.typical_price();
        let vol = bar.volume;

        self.cum_tp_vol += tp * vol;
        self.cum_vol += vol;

        if self.cum_vol.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let vwap = self
            .cum_tp_vol
            .checked_div(self.cum_vol)
            .ok_or(FinError::ArithmeticOverflow)?;

        if vwap.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        self.ready = true;

        let pct = (bar.close - vwap)
            .checked_div(vwap)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
        self.cum_tp_vol = Decimal::ZERO;
        self.cum_vol = Decimal::ZERO;
        self.ready = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, high: &str, low: &str, close: &str, vol: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(open.parse().unwrap()).unwrap(),
            high: Price::new(high.parse().unwrap()).unwrap(),
            low: Price::new(low.parse().unwrap()).unwrap(),
            close: Price::new(close.parse().unwrap()).unwrap(),
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cv_pct_zero_volume_unavailable() {
        let mut cv = CloseToVwapPct::new("cv").unwrap();
        let v = cv.update_bar(&bar("100", "110", "90", "105", "0")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_cv_pct_close_at_vwap_returns_zero() {
        let mut cv = CloseToVwapPct::new("cv").unwrap();
        // typical_price = (110 + 90 + 100) / 3 = 100; close = 100 = vwap → 0%
        let v = cv.update_bar(&bar("100", "110", "90", "100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cv_pct_close_above_vwap_positive() {
        let mut cv = CloseToVwapPct::new("cv").unwrap();
        // tp = (110+90+100)/3 = 100, vwap = 100, close = 110 → +10%
        let v = cv.update_bar(&bar("100", "110", "90", "110", "1000")).unwrap();
        if let SignalValue::Scalar(pct) = v {
            assert!(pct > dec!(0), "above-VWAP close should be positive: {pct}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cv_pct_close_below_vwap_negative() {
        let mut cv = CloseToVwapPct::new("cv").unwrap();
        // tp = (110+90+100)/3 = 100, vwap = 100, close = 90 → -10%
        let v = cv.update_bar(&bar("100", "110", "90", "90", "1000")).unwrap();
        if let SignalValue::Scalar(pct) = v {
            assert!(pct < dec!(0), "below-VWAP close should be negative: {pct}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cv_pct_ready_after_bar_with_volume() {
        let mut cv = CloseToVwapPct::new("cv").unwrap();
        cv.update_bar(&bar("100", "110", "90", "105", "500")).unwrap();
        assert!(cv.is_ready());
    }

    #[test]
    fn test_cv_pct_reset() {
        let mut cv = CloseToVwapPct::new("cv").unwrap();
        cv.update_bar(&bar("100", "110", "90", "105", "500")).unwrap();
        assert!(cv.is_ready());
        cv.reset();
        assert!(!cv.is_ready());
    }
}
