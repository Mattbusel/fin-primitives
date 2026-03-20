//! Open-Close Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Open-Close Ratio — smoothed ratio of the intra-bar close-to-open return.
///
/// ```text
/// raw_t    = (close_t − open_t) / open_t × 100
/// output   = SMA(raw, period)
/// ```
///
/// Positive values indicate bars predominantly close above their open (bullish pressure).
/// Negative values indicate bars that close below their open (bearish pressure).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenCloseRatio;
/// use fin_primitives::signals::Signal;
///
/// let ocr = OpenCloseRatio::new("ocr", 10).unwrap();
/// assert_eq!(ocr.period(), 10);
/// ```
pub struct OpenCloseRatio {
    name: String,
    period: usize,
    raws: VecDeque<Decimal>,
}

impl OpenCloseRatio {
    /// Creates a new `OpenCloseRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            raws: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for OpenCloseRatio {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let raw = if bar.open.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - bar.open) / bar.open * Decimal::from(100u32)
        };

        self.raws.push_back(raw);
        if self.raws.len() > self.period { self.raws.pop_front(); }
        if self.raws.len() < self.period { return Ok(SignalValue::Unavailable); }

        let sma = self.raws.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(sma))
    }

    fn is_ready(&self) -> bool { self.raws.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.raws.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_oc(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let hp = op.max(cp);
        let lp = op.min(cp);
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ocr_invalid() {
        assert!(OpenCloseRatio::new("o", 0).is_err());
    }

    #[test]
    fn test_ocr_unavailable_before_warmup() {
        let mut o = OpenCloseRatio::new("o", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(o.update_bar(&bar_oc("100", "101")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ocr_flat_is_zero() {
        // open = close → raw = 0 → SMA = 0
        let mut o = OpenCloseRatio::new("o", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = o.update_bar(&bar_oc("100", "100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ocr_bullish_positive() {
        // close > open each bar → positive average
        let mut o = OpenCloseRatio::new("o", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = o.update_bar(&bar_oc("100", "102")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "expected > 0, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ocr_bearish_negative() {
        // close < open each bar → negative average
        let mut o = OpenCloseRatio::new("o", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = o.update_bar(&bar_oc("102", "100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(0), "expected < 0, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ocr_reset() {
        let mut o = OpenCloseRatio::new("o", 3).unwrap();
        for _ in 0..5 { o.update_bar(&bar_oc("100", "102")).unwrap(); }
        assert!(o.is_ready());
        o.reset();
        assert!(!o.is_ready());
    }
}
