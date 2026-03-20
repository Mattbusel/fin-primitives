//! Body Fill Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body Fill Ratio — rolling average of `|close - open| / (high - low)`,
/// measuring how much of each bar's range is occupied by the body.
///
/// A high value (near 1.0) indicates strong directional bars with little wicking.
/// A low value indicates doji-like bars with large shadows.
///
/// Bars with zero range contribute `0` to the average.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyFillRatio;
/// use fin_primitives::signals::Signal;
///
/// let bfr = BodyFillRatio::new("bfr", 10).unwrap();
/// assert_eq!(bfr.period(), 10);
/// ```
pub struct BodyFillRatio {
    name: String,
    period: usize,
    ratios: VecDeque<Decimal>,
    sum: Decimal,
}

impl BodyFillRatio {
    /// Constructs a new `BodyFillRatio`.
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

impl Signal for BodyFillRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ratios.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let ratio = if range.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - bar.open).abs() / range
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
    fn test_bfr_invalid_period() {
        assert!(BodyFillRatio::new("bfr", 0).is_err());
    }

    #[test]
    fn test_bfr_unavailable_before_warm_up() {
        let mut bfr = BodyFillRatio::new("bfr", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(bfr.update_bar(&bar("90", "110", "90", "110")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_bfr_full_body_bars() {
        // open=low, close=high → body=range → ratio=1
        let mut bfr = BodyFillRatio::new("bfr", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = bfr.update_bar(&bar("90", "110", "90", "110")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bfr_doji_bars() {
        // open=close → body=0 → ratio=0
        let mut bfr = BodyFillRatio::new("bfr", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = bfr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bfr_reset() {
        let mut bfr = BodyFillRatio::new("bfr", 3).unwrap();
        for _ in 0..3 { bfr.update_bar(&bar("90", "110", "90", "110")).unwrap(); }
        assert!(bfr.is_ready());
        bfr.reset();
        assert!(!bfr.is_ready());
    }
}
