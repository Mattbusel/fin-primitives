//! Cumulative Delta indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Cumulative Delta — rolling sum of bar delta (`close - open`) over the last
/// `period` bars.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CumulativeDelta;
/// use fin_primitives::signals::Signal;
///
/// let cd = CumulativeDelta::new("cd", 10).unwrap();
/// assert_eq!(cd.period(), 10);
/// ```
pub struct CumulativeDelta {
    name: String,
    period: usize,
    deltas: VecDeque<Decimal>,
}

impl CumulativeDelta {
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self { name: name.into(), period, deltas: VecDeque::with_capacity(period) })
    }
}

impl Signal for CumulativeDelta {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.deltas.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let delta = bar.net_move();
        self.deltas.push_back(delta);
        if self.deltas.len() > self.period { self.deltas.pop_front(); }
        if self.deltas.len() < self.period { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(self.deltas.iter().sum()))
    }

    fn reset(&mut self) { self.deltas.clear(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp, low: op, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cd_invalid() { assert!(CumulativeDelta::new("c", 0).is_err()); }

    #[test]
    fn test_cd_unavailable() {
        let mut cd = CumulativeDelta::new("c", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(cd.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_cd_sum() {
        let mut cd = CumulativeDelta::new("c", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 { last = cd.update_bar(&bar("100", "105")).unwrap(); }
        assert_eq!(last, SignalValue::Scalar(dec!(15)));
    }

    #[test]
    fn test_cd_mixed() {
        let mut cd = CumulativeDelta::new("c", 3).unwrap();
        cd.update_bar(&bar("100", "105")).unwrap();
        cd.update_bar(&bar("105", "102")).unwrap();
        let last = cd.update_bar(&bar("100", "102")).unwrap();
        assert_eq!(last, SignalValue::Scalar(dec!(4)));
    }

    #[test]
    fn test_cd_reset() {
        let mut cd = CumulativeDelta::new("c", 3).unwrap();
        for _ in 0..3 { cd.update_bar(&bar("100", "105")).unwrap(); }
        cd.reset();
        assert!(!cd.is_ready());
    }
}
