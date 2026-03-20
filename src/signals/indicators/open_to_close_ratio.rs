//! Open-to-Close Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Open-to-Close Ratio — rolling average of `(close - open) / (high - low)`.
///
/// Captures directional body strength normalized by the total range.
/// Ranges from -1 (fully bearish body, no wicks) to +1 (fully bullish body, no wicks).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
/// Bars with zero range contribute `0` to the average.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenToCloseRatio;
/// use fin_primitives::signals::Signal;
///
/// let ocr = OpenToCloseRatio::new("ocr", 10).unwrap();
/// assert_eq!(ocr.period(), 10);
/// ```
pub struct OpenToCloseRatio {
    name: String,
    period: usize,
    ratios: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenToCloseRatio {
    /// Constructs a new `OpenToCloseRatio`.
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
            ratios: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for OpenToCloseRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ratios.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let ratio = if range.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - bar.open) / range
        };

        self.ratios.push_back(ratio);
        self.sum += ratio;
        if self.ratios.len() > self.period {
            self.sum -= self.ratios.pop_front().unwrap();
        }

        if self.ratios.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let nd = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / nd))
    }

    fn reset(&mut self) {
        self.ratios.clear();
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
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
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
    fn test_ocr_invalid_period() {
        assert!(OpenToCloseRatio::new("ocr", 0).is_err());
    }

    #[test]
    fn test_ocr_unavailable_before_warm_up() {
        let mut ocr = OpenToCloseRatio::new("ocr", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(ocr.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ocr_fully_bullish_bars() {
        // open=low, close=high → ratio = 1 each bar
        let mut ocr = OpenToCloseRatio::new("ocr", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = ocr.update_bar(&bar("90", "110", "90", "110")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ocr_fully_bearish_bars() {
        // open=high, close=low → ratio = -1 each bar
        let mut ocr = OpenToCloseRatio::new("ocr", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = ocr.update_bar(&bar("110", "110", "90", "90")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_ocr_reset() {
        let mut ocr = OpenToCloseRatio::new("ocr", 3).unwrap();
        for _ in 0..3 { ocr.update_bar(&bar("100", "110", "90", "105")).unwrap(); }
        assert!(ocr.is_ready());
        ocr.reset();
        assert!(!ocr.is_ready());
    }
}
