//! Signed Body Ratio indicator.
//!
//! Rolling mean of `(close - open) / range`, measuring average directional
//! commitment as a fraction of the bar's range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Signed Body Ratio — rolling mean of `(close - open) / (high - low)`.
///
/// For each bar:
/// ```text
/// sbr[i] = (close[i] - open[i]) / (high[i] - low[i])   when high > low
///        = 0                                            when high == low
/// ```
///
/// Unlike `BodyWidthRatio` (which uses `|close - open|` — unsigned), this
/// preserves direction:
/// - **+1**: the entire range is a bullish body closing at the high.
/// - **-1**: the entire range is a bearish body closing at the low.
/// - **0**: the body is zero (doji), or bullish and bearish contributions cancel.
///
/// The rolling mean smooths noise and reveals persistent directional conviction.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars are collected.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SignedBodyRatio;
/// use fin_primitives::signals::Signal;
/// let sbr = SignedBodyRatio::new("sbr_14", 14).unwrap();
/// assert_eq!(sbr.period(), 14);
/// ```
pub struct SignedBodyRatio {
    name: String,
    period: usize,
    values: VecDeque<Decimal>,
    sum: Decimal,
}

impl SignedBodyRatio {
    /// Constructs a new `SignedBodyRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            values: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for SignedBodyRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.values.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let sbr = if range.is_zero() {
            Decimal::ZERO
        } else {
            let body = bar.close - bar.open;
            body.checked_div(range).ok_or(FinError::ArithmeticOverflow)?
        };

        self.sum += sbr;
        self.values.push_back(sbr);
        if self.values.len() > self.period {
            let removed = self.values.pop_front().unwrap();
            self.sum -= removed;
        }

        if self.values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mean = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(mean))
    }

    fn reset(&mut self) {
        self.values.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_sbr_invalid_period() {
        assert!(SignedBodyRatio::new("sbr", 0).is_err());
    }

    #[test]
    fn test_sbr_unavailable_during_warmup() {
        let mut sbr = SignedBodyRatio::new("sbr", 3).unwrap();
        assert_eq!(sbr.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sbr.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!sbr.is_ready());
    }

    #[test]
    fn test_sbr_full_bullish_body_one() {
        // open=low, close=high → body=range → sbr=1
        let mut sbr = SignedBodyRatio::new("sbr", 3).unwrap();
        for _ in 0..3 { sbr.update_bar(&bar("90", "110", "90", "110")).unwrap(); }
        if let SignalValue::Scalar(v) = sbr.update_bar(&bar("90", "110", "90", "110")).unwrap() {
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sbr_full_bearish_body_minus_one() {
        // open=high, close=low → body=-range → sbr=-1
        let mut sbr = SignedBodyRatio::new("sbr", 3).unwrap();
        for _ in 0..3 { sbr.update_bar(&bar("110", "110", "90", "90")).unwrap(); }
        if let SignalValue::Scalar(v) = sbr.update_bar(&bar("110", "110", "90", "90")).unwrap() {
            assert_eq!(v, dec!(-1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sbr_doji_zero() {
        // open=close: body=0 → sbr=0 always
        let mut sbr = SignedBodyRatio::new("sbr", 2).unwrap();
        sbr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        if let SignalValue::Scalar(v) = sbr.update_bar(&bar("100", "110", "90", "100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sbr_flat_bar_zero() {
        let mut sbr = SignedBodyRatio::new("sbr", 2).unwrap();
        sbr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        if let SignalValue::Scalar(v) = sbr.update_bar(&bar("100", "100", "100", "100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sbr_reset() {
        let mut sbr = SignedBodyRatio::new("sbr", 2).unwrap();
        sbr.update_bar(&bar("90", "110", "90", "110")).unwrap();
        sbr.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert!(sbr.is_ready());
        sbr.reset();
        assert!(!sbr.is_ready());
    }
}
