//! Close-to-Open indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close-to-Open — rolling mean of overnight gap: (open_t − close_{t−1}) / close_{t−1} × 100.
///
/// ```text
/// gap_t  = (open_t − close_{t−1}) / close_{t−1} × 100
/// output = mean(gap, period)
/// ```
///
/// Positive output indicates persistent gap-up openings; negative gap-downs.
/// Useful for detecting overnight drift patterns and pre-market sentiment.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseToOpen;
/// use fin_primitives::signals::Signal;
///
/// let cto = CloseToOpen::new("cto", 10).unwrap();
/// assert_eq!(cto.period(), 10);
/// ```
pub struct CloseToOpen {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    gaps: VecDeque<Decimal>,
}

impl CloseToOpen {
    /// Creates a new `CloseToOpen`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            gaps: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseToOpen {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let gap = (bar.open - pc) / pc * Decimal::from(100u32);
                self.gaps.push_back(gap);
                if self.gaps.len() > self.period { self.gaps.pop_front(); }
            }
        }
        self.prev_close = Some(bar.close);

        if self.gaps.len() < self.period { return Ok(SignalValue::Unavailable); }

        let avg = self.gaps.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool { self.gaps.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.prev_close = None;
        self.gaps.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_oc(o: &str, c: &str) -> OhlcvBar {
        let ov: rust_decimal::Decimal = o.parse().unwrap();
        let cv: rust_decimal::Decimal = c.parse().unwrap();
        let op = Price::new(ov).unwrap();
        let cp = Price::new(cv).unwrap();
        let hp = Price::new(ov.max(cv)).unwrap();
        let lp = Price::new(ov.min(cv)).unwrap();
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
    fn test_cto_invalid() {
        assert!(CloseToOpen::new("c", 0).is_err());
    }

    #[test]
    fn test_cto_unavailable_before_warmup() {
        let mut c = CloseToOpen::new("c", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(c.update_bar(&bar_oc("100", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_cto_no_gap_is_zero() {
        // open always equals prev close → gap = 0 → mean = 0
        let mut c = CloseToOpen::new("c", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..6 { last = c.update_bar(&bar_oc("100", "100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_cto_gap_up_positive() {
        // close=100, next open=105 → gap = 5% → mean = 5
        let mut c = CloseToOpen::new("c", 3).unwrap();
        c.update_bar(&bar_oc("100", "100")).unwrap();
        for _ in 0..3 { c.update_bar(&bar_oc("105", "100")).unwrap(); }
        // After 4 total bars, 3 gaps each = 5%
        if let SignalValue::Scalar(v) = c.update_bar(&bar_oc("105", "100")).unwrap() {
            assert_eq!(v, dec!(5));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_cto_reset() {
        let mut c = CloseToOpen::new("c", 3).unwrap();
        for _ in 0..6 { c.update_bar(&bar_oc("100", "100")).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
