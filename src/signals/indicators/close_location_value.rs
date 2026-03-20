//! Close Location Value indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Location Value (CLV) — where the close falls within the bar's range.
///
/// ```text
/// CLV_t = (close_t − low_t − (high_t − close_t)) / (high_t − low_t)
///       = (2 × close_t − high_t − low_t) / (high_t − low_t)
/// output = mean(CLV, period)
/// ```
///
/// Ranges from -1 (close at low) to +1 (close at high).
/// A positive mean indicates consistent bullish closes; negative bearish.
/// Returns 0 for doji bars where high == low.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseLocationValue;
/// use fin_primitives::signals::Signal;
///
/// let clv = CloseLocationValue::new("clv", 14).unwrap();
/// assert_eq!(clv.period(), 14);
/// ```
pub struct CloseLocationValue {
    name: String,
    period: usize,
    clvs: VecDeque<Decimal>,
}

impl CloseLocationValue {
    /// Creates a new `CloseLocationValue`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            clvs: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseLocationValue {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let clv = if range.is_zero() {
            Decimal::ZERO
        } else {
            (Decimal::from(2u32) * bar.close - bar.range()) / range
        };

        self.clvs.push_back(clv);
        if self.clvs.len() > self.period { self.clvs.pop_front(); }
        if self.clvs.len() < self.period { return Ok(SignalValue::Unavailable); }

        let avg = self.clvs.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool { self.clvs.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.clvs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hlc(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn bar(c: &str) -> OhlcvBar { bar_hlc(c, c, c) }

    #[test]
    fn test_clv_invalid() {
        assert!(CloseLocationValue::new("c", 0).is_err());
    }

    #[test]
    fn test_clv_unavailable_before_warmup() {
        let mut c = CloseLocationValue::new("c", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_clv_close_at_high_is_one() {
        // close = high → CLV = (2*h - h - l)/(h-l) = (h-l)/(h-l) = 1
        let mut c = CloseLocationValue::new("c", 1).unwrap();
        if let SignalValue::Scalar(v) = c.update_bar(&bar_hlc("110", "90", "110")).unwrap() {
            assert_eq!(v, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_clv_close_at_low_is_minus_one() {
        let mut c = CloseLocationValue::new("c", 1).unwrap();
        if let SignalValue::Scalar(v) = c.update_bar(&bar_hlc("110", "90", "90")).unwrap() {
            assert_eq!(v, dec!(-1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_clv_close_at_midpoint_is_zero() {
        let mut c = CloseLocationValue::new("c", 1).unwrap();
        if let SignalValue::Scalar(v) = c.update_bar(&bar_hlc("110", "90", "100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_clv_doji_is_zero() {
        // high == low == close → range = 0 → CLV = 0
        let mut c = CloseLocationValue::new("c", 1).unwrap();
        if let SignalValue::Scalar(v) = c.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_clv_rolling_average() {
        // bar1 CLV=1 (close at high), bar2 CLV=-1 (close at low) → avg = 0
        let mut c = CloseLocationValue::new("c", 2).unwrap();
        c.update_bar(&bar_hlc("110", "90", "110")).unwrap();
        if let SignalValue::Scalar(v) = c.update_bar(&bar_hlc("110", "90", "90")).unwrap() {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_clv_reset() {
        let mut c = CloseLocationValue::new("c", 3).unwrap();
        for _ in 0..5 { c.update_bar(&bar("100")).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
