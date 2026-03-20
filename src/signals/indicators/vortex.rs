//! Vortex Indicator (VI+ / VI-) by Etienne Botes and Douglas Siepman.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Vortex Indicator — computes VI+ (upward movement) as the primary scalar output.
///
/// ```text
/// VM+[i] = |high[i] - low[i-1]|
/// VM-[i] = |low[i]  - high[i-1]|
/// TR[i]  = max(high[i], prev_close) - min(low[i], prev_close)
///
/// VI+    = sum(VM+, n) / sum(TR, n)
/// VI-    = sum(VM-, n) / sum(TR, n)
/// ```
///
/// `Signal::update()` returns VI+ as the primary scalar. Access VI- via [`Vortex::vi_minus`].
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Vortex;
/// use fin_primitives::signals::Signal;
///
/// let vortex = Vortex::new("vx", 14).unwrap();
/// assert_eq!(vortex.period(), 14);
/// ```
pub struct Vortex {
    name: String,
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    prev_close: Option<Decimal>,
    vm_plus:  VecDeque<Decimal>,
    vm_minus: VecDeque<Decimal>,
    trs:      VecDeque<Decimal>,
    last_vi_minus: Option<Decimal>,
}

impl Vortex {
    /// Constructs a new `Vortex` with the given period.
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
            prev_high: None,
            prev_low: None,
            prev_close: None,
            vm_plus:  VecDeque::with_capacity(period),
            vm_minus: VecDeque::with_capacity(period),
            trs:      VecDeque::with_capacity(period),
            last_vi_minus: None,
        })
    }

    /// Returns the latest VI- value, or `None` if not yet ready.
    pub fn vi_minus(&self) -> Option<Decimal> {
        self.last_vi_minus
    }
}

impl Signal for Vortex {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let (prev_high, prev_low, prev_close) = match (self.prev_high, self.prev_low, self.prev_close) {
            (Some(h), Some(l), Some(c)) => (h, l, c),
            _ => {
                self.prev_high  = Some(bar.high);
                self.prev_low   = Some(bar.low);
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
        };

        let vm_p = (bar.high - prev_low).abs();
        let vm_m = (bar.low  - prev_high).abs();
        let true_high = bar.high.max(prev_close);
        let true_low  = bar.low.min(prev_close);
        let tr = true_high - true_low;

        self.vm_plus.push_back(vm_p);
        self.vm_minus.push_back(vm_m);
        self.trs.push_back(tr);
        if self.vm_plus.len() > self.period {
            self.vm_plus.pop_front();
            self.vm_minus.pop_front();
            self.trs.pop_front();
        }

        self.prev_high  = Some(bar.high);
        self.prev_low   = Some(bar.low);
        self.prev_close = Some(bar.close);

        if self.vm_plus.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum_vm_p: Decimal = self.vm_plus.iter().sum();
        let sum_vm_m: Decimal = self.vm_minus.iter().sum();
        let sum_tr:   Decimal = self.trs.iter().sum();

        if sum_tr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let vi_plus  = sum_vm_p / sum_tr;
        let vi_minus = sum_vm_m / sum_tr;
        self.last_vi_minus = Some(vi_minus);
        Ok(SignalValue::Scalar(vi_plus))
    }

    fn is_ready(&self) -> bool {
        self.last_vi_minus.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_high  = None;
        self.prev_low   = None;
        self.prev_close = None;
        self.vm_plus.clear();
        self.vm_minus.clear();
        self.trs.clear();
        self.last_vi_minus = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cl, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vortex_period_zero_fails() {
        assert!(Vortex::new("vx", 0).is_err());
    }

    #[test]
    fn test_vortex_unavailable_before_period() {
        let mut vx = Vortex::new("vx", 3).unwrap();
        assert_eq!(vx.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(vx.update_bar(&bar("115", "95", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!vx.is_ready());
    }

    #[test]
    fn test_vortex_ready_after_period() {
        let mut vx = Vortex::new("vx", 3).unwrap();
        for _ in 0..4 {
            vx.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(vx.is_ready());
        assert!(vx.vi_minus().is_some());
    }

    #[test]
    fn test_vortex_reset() {
        let mut vx = Vortex::new("vx", 3).unwrap();
        for _ in 0..5 { vx.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(vx.is_ready());
        vx.reset();
        assert!(!vx.is_ready());
        assert!(vx.vi_minus().is_none());
    }
}
