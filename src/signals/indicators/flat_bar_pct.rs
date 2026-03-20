//! Flat Bar Percent indicator -- rolling % of bars where close equals previous close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Flat Bar Percent -- rolling percentage of bars where `close == prev_close`.
///
/// High values indicate a stagnant, illiquid, or range-bound market where the price
/// repeatedly fails to move. Useful for filtering signals in low-activity periods.
///
/// ```text
/// flat[t]  = 1 if close[t] == close[t-1], else 0
/// pct[t]   = sum(flat, period) / period * 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` comparisons have been made
/// (requires `period + 1` bars).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::FlatBarPct;
/// use fin_primitives::signals::Signal;
/// let fb = FlatBarPct::new("fb", 10).unwrap();
/// assert_eq!(fb.period(), 10);
/// ```
pub struct FlatBarPct {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl FlatBarPct {
    /// Constructs a new `FlatBarPct`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for FlatBarPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let flat: u8 = if bar.close == pc { 1 } else { 0 };
            self.window.push_back(flat);
            self.count += flat as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() { self.count -= old as usize; }
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let pct = Decimal::from(self.count as u32)
            / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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

    #[test]
    fn test_fb_period_0_error() { assert!(FlatBarPct::new("fb", 0).is_err()); }

    #[test]
    fn test_fb_unavailable_before_period() {
        let mut fb = FlatBarPct::new("fb", 3).unwrap();
        assert_eq!(fb.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_fb_all_flat_is_100() {
        let mut fb = FlatBarPct::new("fb", 3).unwrap();
        fb.update_bar(&bar("100")).unwrap(); // no comparison
        fb.update_bar(&bar("100")).unwrap(); // flat -> window=[1]
        fb.update_bar(&bar("100")).unwrap(); // flat -> window=[1,1]
        let v = fb.update_bar(&bar("100")).unwrap(); // flat -> window=[1,1,1] -> 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_fb_no_flat_is_0() {
        let mut fb = FlatBarPct::new("fb", 3).unwrap();
        fb.update_bar(&bar("100")).unwrap();
        fb.update_bar(&bar("101")).unwrap();
        fb.update_bar(&bar("102")).unwrap();
        let v = fb.update_bar(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_fb_half_flat() {
        let mut fb = FlatBarPct::new("fb", 2).unwrap();
        fb.update_bar(&bar("100")).unwrap();
        fb.update_bar(&bar("100")).unwrap(); // flat -> window=[1]
        let v = fb.update_bar(&bar("101")).unwrap(); // not flat -> window=[1,0] -> 50%
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_fb_reset() {
        let mut fb = FlatBarPct::new("fb", 3).unwrap();
        for p in ["100", "100", "100", "100"] { fb.update_bar(&bar(p)).unwrap(); }
        assert!(fb.is_ready());
        fb.reset();
        assert!(!fb.is_ready());
    }
}
