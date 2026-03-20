//! Return Persistence indicator -- rolling fraction of returns with the same sign.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Return Persistence -- rolling fraction of consecutive return-sign pairs where
/// the current return has the same sign as the previous return.
///
/// Measures momentum persistence: high values mean returns tend to continue in the
/// same direction (trending); low values mean frequent reversals (mean-reverting).
///
/// ```text
/// ret[t]        = close[t] - close[t-1]
/// persist[t]    = 1 if sign(ret[t]) == sign(ret[t-1]), else 0
/// pct[t]        = sum(persist, period) / period * 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 2` bars have been seen
/// (need at least `period` sign-pair comparisons).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ReturnPersistence;
/// use fin_primitives::signals::Signal;
/// let rp = ReturnPersistence::new("rp", 10).unwrap();
/// assert_eq!(rp.period(), 10);
/// ```
pub struct ReturnPersistence {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    prev_ret: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl ReturnPersistence {
    /// Constructs a new `ReturnPersistence`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            prev_ret: None,
            window: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for ReturnPersistence {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let ret = bar.close - pc;
            if let Some(pr) = self.prev_ret {
                // Same sign (both positive, both negative, or both zero)
                let persist: u8 = if (ret > Decimal::ZERO && pr > Decimal::ZERO)
                    || (ret < Decimal::ZERO && pr < Decimal::ZERO)
                    || (ret.is_zero() && pr.is_zero())
                { 1 } else { 0 };
                self.window.push_back(persist);
                self.count += persist as usize;
                if self.window.len() > self.period {
                    if let Some(old) = self.window.pop_front() { self.count -= old as usize; }
                }
            }
            self.prev_ret = Some(ret);
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
        self.prev_ret = None;
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
    fn test_rp_period_0_error() { assert!(ReturnPersistence::new("rp", 0).is_err()); }

    #[test]
    fn test_rp_unavailable_before_period() {
        let mut rp = ReturnPersistence::new("rp", 3).unwrap();
        assert_eq!(rp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(rp.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rp_trending_series_is_100() {
        // Monotone rising -> all returns same sign -> 100% persistence
        let mut rp = ReturnPersistence::new("rp", 3).unwrap();
        rp.update_bar(&bar("100")).unwrap();
        rp.update_bar(&bar("101")).unwrap(); // ret=+1, no prev_ret yet
        rp.update_bar(&bar("102")).unwrap(); // ret=+1, persist=1 -> window=[1]
        rp.update_bar(&bar("103")).unwrap(); // ret=+1, persist=1 -> window=[1,1]
        let v = rp.update_bar(&bar("104")).unwrap(); // ret=+1, persist=1 -> window=[1,1,1] -> 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_rp_alternating_series_is_0() {
        // Alternating returns -> no consecutive same-sign pairs -> 0%
        let mut rp = ReturnPersistence::new("rp", 3).unwrap();
        rp.update_bar(&bar("100")).unwrap();
        rp.update_bar(&bar("101")).unwrap(); // +1
        rp.update_bar(&bar("100")).unwrap(); // -1 -> persist=0, window=[0]
        rp.update_bar(&bar("101")).unwrap(); // +1 -> persist=0, window=[0,0]
        let v = rp.update_bar(&bar("100")).unwrap(); // -1 -> persist=0, window=[0,0,0] -> 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rp_reset() {
        let mut rp = ReturnPersistence::new("rp", 3).unwrap();
        for p in ["100", "101", "102", "103", "104"] { rp.update_bar(&bar(p)).unwrap(); }
        assert!(rp.is_ready());
        rp.reset();
        assert!(!rp.is_ready());
    }
}
