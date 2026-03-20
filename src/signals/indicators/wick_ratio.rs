//! Wick Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Wick Ratio — rolling mean of the wick-to-body ratio.
///
/// ```text
/// wick_t  = (high_t − low_t) − |close_t − open_t|   (total wick length)
/// body_t  = |close_t − open_t|
///
/// ratio_t = wick_t / body_t   (if body > 0)
///         = 0                 (if body = 0, pure doji)
///
/// output  = mean(ratio, period)
/// ```
///
/// High values indicate long wicks relative to bodies (indecision/rejection).
/// Low values indicate dominant directional candles with little wick.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WickRatio;
/// use fin_primitives::signals::Signal;
///
/// let wr = WickRatio::new("wr", 14).unwrap();
/// assert_eq!(wr.period(), 14);
/// ```
pub struct WickRatio {
    name: String,
    period: usize,
    ratios: VecDeque<Decimal>,
}

impl WickRatio {
    /// Creates a new `WickRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            ratios: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for WickRatio {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let body = (bar.net_move()).abs();
        let wick = range - body;
        let ratio = if body.is_zero() { Decimal::ZERO } else { wick / body };

        self.ratios.push_back(ratio);
        if self.ratios.len() > self.period { self.ratios.pop_front(); }
        if self.ratios.len() < self.period { return Ok(SignalValue::Unavailable); }

        let avg = self.ratios.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool { self.ratios.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.ratios.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hloc(h: &str, l: &str, o: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let op = Price::new(o.parse().unwrap()).unwrap();
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

    fn full_body_bar() -> OhlcvBar { bar_hloc("110", "90", "90", "110") }
    fn doji_bar() -> OhlcvBar { bar_hloc("110", "90", "100", "100") }

    #[test]
    fn test_wr_invalid() {
        assert!(WickRatio::new("w", 0).is_err());
    }

    #[test]
    fn test_wr_unavailable_before_warmup() {
        let mut w = WickRatio::new("w", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(w.update_bar(&full_body_bar()).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_wr_full_body_is_zero() {
        // h=110, l=90, o=90, c=110 → body=20, wick=range-body=20-20=0 → ratio=0
        let mut w = WickRatio::new("w", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = w.update_bar(&full_body_bar()).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_wr_doji_is_zero() {
        // doji: body=0 → ratio=0
        let mut w = WickRatio::new("w", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = w.update_bar(&doji_bar()).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_wr_small_body_high_wick() {
        // h=115, l=85, o=99, c=101 → body=2, range=30, wick=28 → ratio=14
        let mut w = WickRatio::new("w", 1).unwrap();
        if let SignalValue::Scalar(v) = w.update_bar(&bar_hloc("115", "85", "99", "101")).unwrap() {
            assert_eq!(v, dec!(14));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_wr_non_negative() {
        let mut w = WickRatio::new("w", 3).unwrap();
        for bar in [full_body_bar(), doji_bar(), bar_hloc("115", "85", "99", "101")] {
            if let SignalValue::Scalar(v) = w.update_bar(&bar).unwrap() {
                assert!(v >= dec!(0), "expected non-negative, got {v}");
            }
        }
    }

    #[test]
    fn test_wr_reset() {
        let mut w = WickRatio::new("w", 3).unwrap();
        for _ in 0..5 { w.update_bar(&full_body_bar()).unwrap(); }
        assert!(w.is_ready());
        w.reset();
        assert!(!w.is_ready());
    }
}
