//! High of Period indicator -- rolling N-bar highest high.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High of Period -- the highest high seen over the last `period` bars.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighOfPeriod;
/// use fin_primitives::signals::Signal;
/// let h = HighOfPeriod::new("hop", 20).unwrap();
/// assert_eq!(h.period(), 20);
/// ```
pub struct HighOfPeriod {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl HighOfPeriod {
    /// Constructs a new `HighOfPeriod`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self { name: name.into(), period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for HighOfPeriod {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.high);
        if self.window.len() > self.period { self.window.pop_front(); }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        let max = self.window.iter().copied().fold(Decimal::MIN, Decimal::max);
        Ok(SignalValue::Scalar(max))
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str) -> OhlcvBar {
        let p = Price::new(h.parse().unwrap()).unwrap();
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
    fn test_hop_period_0_error() { assert!(HighOfPeriod::new("h", 0).is_err()); }

    #[test]
    fn test_hop_unavailable_before_period() {
        let mut h = HighOfPeriod::new("h", 3).unwrap();
        assert_eq!(h.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_hop_returns_max() {
        let mut h = HighOfPeriod::new("h", 3).unwrap();
        h.update_bar(&bar("90")).unwrap();
        h.update_bar(&bar("110")).unwrap();
        let v = h.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(110)));
    }

    #[test]
    fn test_hop_rolls_out_old_max() {
        let mut h = HighOfPeriod::new("h", 3).unwrap();
        h.update_bar(&bar("150")).unwrap(); // will roll out
        h.update_bar(&bar("90")).unwrap();
        h.update_bar(&bar("95")).unwrap(); // window full, max=150
        let v = h.update_bar(&bar("100")).unwrap(); // 150 rolls out, max=100
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_hop_reset() {
        let mut h = HighOfPeriod::new("h", 2).unwrap();
        h.update_bar(&bar("100")).unwrap();
        h.update_bar(&bar("110")).unwrap();
        assert!(h.is_ready());
        h.reset();
        assert!(!h.is_ready());
    }
}
