//! Rolling Min Return indicator -- lowest close-to-close return in a window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Min Return -- the most negative single-bar return (worst bar) within the
/// last `period` bars.
///
/// ```text
/// return[t]     = close[t] - close[t-1]
/// min_return[t] = min(return[t-period+1..t])
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` returns have been accumulated
/// (needs `period + 1` closes).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingMinReturn;
/// use fin_primitives::signals::Signal;
/// let rmr = RollingMinReturn::new("rmr", 10).unwrap();
/// assert_eq!(rmr.period(), 10);
/// ```
pub struct RollingMinReturn {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
}

impl RollingMinReturn {
    /// Constructs a new `RollingMinReturn`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            returns: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RollingMinReturn {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ret = match self.prev_close {
            None => { self.prev_close = Some(bar.close); return Ok(SignalValue::Unavailable); }
            Some(pc) => bar.close - pc,
        };
        self.prev_close = Some(bar.close);
        self.returns.push_back(ret);
        if self.returns.len() > self.period { self.returns.pop_front(); }
        if self.returns.len() < self.period { return Ok(SignalValue::Unavailable); }
        let min = self.returns.iter().copied().fold(Decimal::MAX, Decimal::min);
        Ok(SignalValue::Scalar(min))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.returns.clear();
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
    fn test_rminr_period_0_error() { assert!(RollingMinReturn::new("r", 0).is_err()); }

    #[test]
    fn test_rminr_unavailable_before_warmup() {
        let mut r = RollingMinReturn::new("r", 3).unwrap();
        assert_eq!(r.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(r.update_bar(&bar("99")).unwrap(), SignalValue::Unavailable);
        assert_eq!(r.update_bar(&bar("98")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rminr_min_return_correct() {
        let mut r = RollingMinReturn::new("r", 3).unwrap();
        r.update_bar(&bar("100")).unwrap();
        r.update_bar(&bar("90")).unwrap();  // -10
        r.update_bar(&bar("95")).unwrap();  // +5
        let v = r.update_bar(&bar("93")).unwrap(); // -2, window=[-10,5,-2], min=-10
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_rminr_reset() {
        let mut r = RollingMinReturn::new("r", 2).unwrap();
        r.update_bar(&bar("100")).unwrap();
        r.update_bar(&bar("90")).unwrap();
        r.update_bar(&bar("80")).unwrap();
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
