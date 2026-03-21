//! Wicks-vs-Body Ratio indicator.
//!
//! Compares total wick length to body size, measuring the balance between
//! directional commitment (body) and price rejection (wicks).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Wicks-vs-Body Ratio: `(upper_wick + lower_wick) / body_size`.
///
/// Quantifies how much of the bar's internal movement was "rejected" relative
/// to how much was "committed" to directional movement:
///
/// - **High ratio** (wicks >> body): indecision, strong rejection — potential
///   reversal or absorption zone.
/// - **Low ratio** (body >> wicks): strong directional commitment — trending bar.
/// - **Exact 0**: no wicks at all (marubozu-style bar).
///
/// `upper_wick = high - max(open, close)` \
/// `lower_wick = min(open, close) - low` \
/// `body_size  = |close - open|`
///
/// Returns [`SignalValue::Unavailable`] when `body_size == 0` (doji bar, where
/// the ratio is undefined).
///
/// Always ready after the first bar. Period = 1.
///
/// # Errors
/// Returns [`FinError::ArithmeticOverflow`] on arithmetic failure.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WicksVsBodyRatio;
/// use fin_primitives::signals::Signal;
///
/// let wvb = WicksVsBodyRatio::new("wvb").unwrap();
/// assert_eq!(wvb.period(), 1);
/// ```
pub struct WicksVsBodyRatio {
    name: String,
    ready: bool,
}

impl WicksVsBodyRatio {
    /// Constructs a new `WicksVsBodyRatio`.
    ///
    /// # Errors
    /// Never fails; returns `Ok` always.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), ready: false })
    }
}

impl Signal for WicksVsBodyRatio {
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
        self.ready = true;

        let body = bar.body_size();
        if body.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let upper_wick = bar.upper_wick();
        let lower_wick = bar.lower_wick();
        let total_wick = upper_wick + lower_wick;

        let ratio = total_wick
            .checked_div(body)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
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
    fn test_wvb_doji_returns_unavailable() {
        let mut wvb = WicksVsBodyRatio::new("wvb").unwrap();
        // open == close → body = 0
        let v = wvb.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_wvb_no_wicks_returns_zero() {
        let mut wvb = WicksVsBodyRatio::new("wvb").unwrap();
        // open == low, close == high: no wicks (marubozu)
        let v = wvb.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_wvb_correct_ratio() {
        let mut wvb = WicksVsBodyRatio::new("wvb").unwrap();
        // open=95, high=110, low=90, close=100
        // body = 5, upper_wick = 10, lower_wick = 5, total_wick = 15
        // ratio = 15/5 = 3
        let v = wvb.update_bar(&bar("95", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_wvb_bearish_bar() {
        let mut wvb = WicksVsBodyRatio::new("wvb").unwrap();
        // open=105, high=110, low=90, close=100
        // body = 5, upper_wick = 5, lower_wick = 10, total = 15
        // ratio = 3
        let v = wvb.update_bar(&bar("105", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_wvb_ready_after_first_bar() {
        let mut wvb = WicksVsBodyRatio::new("wvb").unwrap();
        wvb.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(wvb.is_ready());
    }

    #[test]
    fn test_wvb_period_is_one() {
        let wvb = WicksVsBodyRatio::new("wvb").unwrap();
        assert_eq!(wvb.period(), 1);
    }

    #[test]
    fn test_wvb_reset() {
        let mut wvb = WicksVsBodyRatio::new("wvb").unwrap();
        wvb.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(wvb.is_ready());
        wvb.reset();
        assert!(!wvb.is_ready());
    }
}
