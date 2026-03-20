//! Volatility Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volatility Ratio — true range of the current bar vs the average true range.
///
/// ```text
/// TR_t  = max(high − low, |high − prev_close|, |low − prev_close|)
/// ATR_t = mean(TR, period)
/// VR    = TR_t / ATR_t
/// ```
///
/// Values > 1 indicate an unusually large bar (potential breakout).
/// Values < 1 indicate an unusually small bar (compression).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityRatio;
/// use fin_primitives::signals::Signal;
///
/// let vr = VolatilityRatio::new("vr", 14).unwrap();
/// assert_eq!(vr.period(), 14);
/// ```
pub struct VolatilityRatio {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    trs: VecDeque<Decimal>,
    current_tr: Option<Decimal>,
}

impl VolatilityRatio {
    /// Creates a new `VolatilityRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            trs: VecDeque::with_capacity(period),
            current_tr: None,
        })
    }

    /// Returns the most recent true range value.
    pub fn current_tr(&self) -> Option<Decimal> { self.current_tr }
}

impl Signal for VolatilityRatio {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = match self.prev_close {
            None => bar.range(),
            Some(pc) => (bar.range())
                .max((bar.high - pc).abs())
                .max((bar.low - pc).abs()),
        };
        self.current_tr = Some(tr);
        self.prev_close = Some(bar.close);

        self.trs.push_back(tr);
        if self.trs.len() > self.period { self.trs.pop_front(); }
        if self.trs.len() < self.period { return Ok(SignalValue::Unavailable); }

        let atr = self.trs.iter().sum::<Decimal>() / Decimal::from(self.period as u32);

        if atr.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ONE));
        }
        Ok(SignalValue::Scalar(tr / atr))
    }

    fn is_ready(&self) -> bool { self.trs.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.prev_close = None;
        self.trs.clear();
        self.current_tr = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

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

    #[test]
    fn test_vr_invalid() {
        assert!(VolatilityRatio::new("v", 0).is_err());
    }

    #[test]
    fn test_vr_unavailable_before_warmup() {
        let mut v = VolatilityRatio::new("v", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vr_flat_is_one() {
        // All flat bars → all TRs = 0 → ATR = 0 → returns 1
        let mut v = VolatilityRatio::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = v.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vr_uniform_bars_is_one() {
        // All bars same range → TR = ATR → VR = 1
        let mut v = VolatilityRatio::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = v.update_bar(&bar_hlc("105", "95", "100")).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vr_spike_above_one() {
        // After stable bars, a spike bar should give VR > 1
        let mut v = VolatilityRatio::new("v", 3).unwrap();
        for _ in 0..5 { v.update_bar(&bar_hlc("101", "99", "100")).unwrap(); }
        if let SignalValue::Scalar(val) = v.update_bar(&bar_hlc("120", "80", "100")).unwrap() {
            assert!(val > dec!(1), "expected VR > 1, got {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vr_reset() {
        let mut v = VolatilityRatio::new("v", 3).unwrap();
        for _ in 0..5 { v.update_bar(&bar("100")).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
        assert!(v.current_tr().is_none());
    }
}
