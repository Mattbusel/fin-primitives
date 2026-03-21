//! Gap-to-Body Ratio indicator.
//!
//! Compares the overnight gap magnitude to the intrabar body size, identifying
//! bars where the gap dominated the session's net directional move.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Gap-to-Body Ratio: `|open - prev_close| / |close - open|`.
///
/// Compares the overnight (or inter-bar) gap magnitude to the body of the
/// current bar. A ratio > 1 means the gap was larger than the bar's own
/// directional move. A ratio near 0 means the session's body dominated.
///
/// - **> 1**: gap-driven bar — the opening gap contributed more than the
///   intrabar move to the session's overall displacement.
/// - **< 1**: body-driven bar — intrabar action exceeded the gap.
/// - **= 0**: no gap; open == prev_close.
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no previous close)
/// or when the current bar's body is zero (doji — division undefined).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] is never triggered; signature kept
/// consistent with other single-bar indicators.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GapBodyRatio;
/// use fin_primitives::signals::Signal;
///
/// let gbr = GapBodyRatio::new("gbr").unwrap();
/// assert_eq!(gbr.period(), 1);
/// assert!(!gbr.is_ready());
/// ```
pub struct GapBodyRatio {
    name: String,
    prev_close: Option<Decimal>,
    seen_bars: usize,
}

impl GapBodyRatio {
    /// Constructs a new `GapBodyRatio`.
    ///
    /// # Errors
    /// Never fails; returns `Ok` always.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_close: None, seen_bars: 0 })
    }
}

impl Signal for GapBodyRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        self.seen_bars >= 2
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.seen_bars += 1;

        let Some(prev) = self.prev_close else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };

        self.prev_close = Some(bar.close);

        let gap = (bar.open - prev).abs();
        let body = (bar.close - bar.open).abs();

        if body.is_zero() {
            // Doji: body undefined; cannot form a meaningful ratio.
            return Ok(SignalValue::Unavailable);
        }

        let ratio = gap
            .checked_div(body)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.seen_bars = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, close: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let high = if c >= o { c } else { o };
        let low = if c <= o { c } else { o };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o,
            high,
            low,
            close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_gbr_first_bar_unavailable() {
        let mut gbr = GapBodyRatio::new("gbr").unwrap();
        assert_eq!(gbr.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!gbr.is_ready());
    }

    #[test]
    fn test_gbr_ready_after_second_bar() {
        let mut gbr = GapBodyRatio::new("gbr").unwrap();
        gbr.update_bar(&bar("100", "105")).unwrap();
        // prev_close = 105, next bar opens at 107
        gbr.update_bar(&bar("107", "110")).unwrap();
        assert!(gbr.is_ready());
    }

    #[test]
    fn test_gbr_doji_returns_unavailable() {
        let mut gbr = GapBodyRatio::new("gbr").unwrap();
        gbr.update_bar(&bar("100", "105")).unwrap();
        // doji: open == close
        let v = gbr.update_bar(&bar("106", "106")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_gbr_no_gap_returns_zero() {
        let mut gbr = GapBodyRatio::new("gbr").unwrap();
        gbr.update_bar(&bar("100", "105")).unwrap();
        // open == prev_close: no gap
        let v = gbr.update_bar(&bar("105", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_gbr_gap_larger_than_body() {
        let mut gbr = GapBodyRatio::new("gbr").unwrap();
        gbr.update_bar(&bar("100", "100")).unwrap();
        // gap = |110 - 100| = 10, body = |112 - 110| = 2, ratio = 5
        let v = gbr.update_bar(&bar("110", "112")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_gbr_body_larger_than_gap() {
        let mut gbr = GapBodyRatio::new("gbr").unwrap();
        gbr.update_bar(&bar("100", "100")).unwrap();
        // gap = |101 - 100| = 1, body = |111 - 101| = 10, ratio = 0.1
        let v = gbr.update_bar(&bar("101", "111")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.1)));
    }

    #[test]
    fn test_gbr_period_is_one() {
        let gbr = GapBodyRatio::new("gbr").unwrap();
        assert_eq!(gbr.period(), 1);
    }

    #[test]
    fn test_gbr_reset() {
        let mut gbr = GapBodyRatio::new("gbr").unwrap();
        gbr.update_bar(&bar("100", "105")).unwrap();
        gbr.update_bar(&bar("106", "110")).unwrap();
        assert!(gbr.is_ready());
        gbr.reset();
        assert!(!gbr.is_ready());
        assert_eq!(gbr.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
    }
}
