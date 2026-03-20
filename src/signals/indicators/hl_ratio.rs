//! High/Low Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High/Low Ratio — ratio of the N-period high to the N-period low.
///
/// ```text
/// HL_ratio = max(high, period) / min(low, period)
/// ```
///
/// Values near 1.0 indicate tight consolidation; large values indicate wide range.
/// Returns `None` when `period_low == 0`.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HlRatio;
/// use fin_primitives::signals::Signal;
///
/// let hr = HlRatio::new("hlr", 20).unwrap();
/// assert_eq!(hr.period(), 20);
/// ```
pub struct HlRatio {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl HlRatio {
    /// Creates a new `HlRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for HlRatio {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period { self.highs.pop_front(); }
        if self.lows.len() > self.period { self.lows.pop_front(); }
        if self.highs.len() < self.period { return Ok(SignalValue::Unavailable); }

        let period_high = self.highs.iter().cloned().max().unwrap();
        let period_low = self.lows.iter().cloned().min().unwrap();

        if period_low.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(period_high / period_low))
    }

    fn is_ready(&self) -> bool { self.highs.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: lp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_hlr_invalid() {
        assert!(HlRatio::new("h", 0).is_err());
    }

    #[test]
    fn test_hlr_unavailable_before_warmup() {
        let mut h = HlRatio::new("h", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(h.update_bar(&bar("105", "95")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_hlr_flat_is_one() {
        // h=l → HL ratio = 100/100 = 1
        let mut h = HlRatio::new("h", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..4 { last = h.update_bar(&bar("100", "100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_hlr_range() {
        // high=110, low=90 → ratio = 110/90 ≈ 1.222...
        let mut h = HlRatio::new("h", 2).unwrap();
        h.update_bar(&bar("110", "90")).unwrap();
        if let SignalValue::Scalar(v) = h.update_bar(&bar("110", "90")).unwrap() {
            assert!(v > dec!(1), "expected > 1, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_hlr_reset() {
        let mut h = HlRatio::new("h", 3).unwrap();
        for _ in 0..5 { h.update_bar(&bar("105", "95")).unwrap(); }
        assert!(h.is_ready());
        h.reset();
        assert!(!h.is_ready());
    }
}
