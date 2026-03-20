//! Rolling Max Return indicator -- highest close-to-close return in a window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Max Return -- the largest single-bar close-to-close return observed
/// within the last `period` bars.
///
/// ```text
/// return[t]     = close[t] - close[t-1]
/// max_return[t] = max(return[t-period+1..t])
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen
/// (need `period + 1` closes to compute `period` returns).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingMaxReturn;
/// use fin_primitives::signals::Signal;
/// let rmr = RollingMaxReturn::new("rmr", 10).unwrap();
/// assert_eq!(rmr.period(), 10);
/// ```
pub struct RollingMaxReturn {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
}

impl RollingMaxReturn {
    /// Constructs a new `RollingMaxReturn`.
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

impl Signal for RollingMaxReturn {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => { self.prev_close = Some(bar.close); return Ok(SignalValue::Unavailable); }
            Some(pc) => bar.close - pc,
        };
        self.prev_close = Some(bar.close);
        self.returns.push_back(result);
        if self.returns.len() > self.period { self.returns.pop_front(); }
        if self.returns.len() < self.period { return Ok(SignalValue::Unavailable); }
        let max = self.returns.iter().copied().fold(Decimal::MIN, Decimal::max);
        Ok(SignalValue::Scalar(max))
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
    fn test_rmr_period_0_error() { assert!(RollingMaxReturn::new("r", 0).is_err()); }

    #[test]
    fn test_rmr_unavailable_before_warmup() {
        let mut r = RollingMaxReturn::new("r", 3).unwrap();
        // first bar: no prev -> Unavailable
        assert_eq!(r.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        // 2nd and 3rd bars: have 1 and 2 returns, < period=3 -> Unavailable
        assert_eq!(r.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert_eq!(r.update_bar(&bar("102")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rmr_max_return_correct() {
        let mut r = RollingMaxReturn::new("r", 3).unwrap();
        r.update_bar(&bar("100")).unwrap(); // seed
        r.update_bar(&bar("110")).unwrap(); // return=+10
        r.update_bar(&bar("105")).unwrap(); // return=-5
        let v = r.update_bar(&bar("108")).unwrap(); // return=+3, window=[10,-5,3], max=10
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_rmr_rolling_drops_old_max() {
        let mut r = RollingMaxReturn::new("r", 3).unwrap();
        r.update_bar(&bar("100")).unwrap();
        r.update_bar(&bar("110")).unwrap(); // +10
        r.update_bar(&bar("111")).unwrap(); // +1
        r.update_bar(&bar("113")).unwrap(); // +2, window=[10,1,2], max=10
        // Now drop +10 from window
        let v = r.update_bar(&bar("114")).unwrap(); // +1, window=[1,2,1], max=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_rmr_reset() {
        let mut r = RollingMaxReturn::new("r", 2).unwrap();
        r.update_bar(&bar("100")).unwrap();
        r.update_bar(&bar("110")).unwrap();
        r.update_bar(&bar("120")).unwrap();
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
