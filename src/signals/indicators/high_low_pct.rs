//! High-Low Percent indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Low Percent — rolling N-period high minus low, expressed as a
/// percentage of the N-period low.
///
/// ```text
/// high_n = max(high, period)
/// low_n  = min(low,  period)
/// output = (high_n − low_n) / low_n × 100
/// ```
///
/// Measures the width of the trading range as a percentage.
/// Higher values indicate a wider range (more volatile period).
/// Returns 0 when the N-period low is zero.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowPct;
/// use fin_primitives::signals::Signal;
///
/// let hlp = HighLowPct::new("hlp", 14).unwrap();
/// assert_eq!(hlp.period(), 14);
/// ```
pub struct HighLowPct {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl HighLowPct {
    /// Creates a new `HighLowPct`.
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

impl Signal for HighLowPct {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period { self.highs.pop_front(); }
        if self.lows.len() > self.period { self.lows.pop_front(); }
        if self.highs.len() < self.period { return Ok(SignalValue::Unavailable); }

        let high_n = self.highs.iter().cloned().max().unwrap();
        let low_n = self.lows.iter().cloned().min().unwrap();

        if low_n.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let pct = (high_n - low_n) / low_n * Decimal::from(100u32);
        Ok(SignalValue::Scalar(pct))
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

    fn bar_hl(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(dec!(100)).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn bar(c: &str) -> OhlcvBar { bar_hl(c, c) }

    #[test]
    fn test_hlp_invalid() {
        assert!(HighLowPct::new("h", 0).is_err());
    }

    #[test]
    fn test_hlp_unavailable_before_warmup() {
        let mut h = HighLowPct::new("h", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(h.update_bar(&bar_hl("110", "90")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_hlp_flat_is_zero() {
        // H=L=100 every bar → range = 0 → pct = 0
        let mut h = HighLowPct::new("h", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = h.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_hlp_known_value() {
        // high_n = 110, low_n = 90 → pct = (110-90)/90 × 100 ≈ 22.22
        let mut h = HighLowPct::new("h", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = h.update_bar(&bar_hl("110", "90")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            let expected = (dec!(110) - dec!(90)) / dec!(90) * dec!(100);
            let diff = (v - expected).abs();
            assert!(diff < dec!(0.001), "expected {expected}, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_hlp_positive() {
        let mut h = HighLowPct::new("h", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = h.update_bar(&bar_hl("115", "85")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "expected positive, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_hlp_reset() {
        let mut h = HighLowPct::new("h", 3).unwrap();
        for _ in 0..5 { h.update_bar(&bar_hl("110", "90")).unwrap(); }
        assert!(h.is_ready());
        h.reset();
        assert!(!h.is_ready());
    }
}
