//! Close Above Prev Close indicator -- rolling % of bars where close > previous close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Above Prev Close -- rolling percentage of bars where close > previous bar close.
///
/// Measures bullish follow-through: a high value (near 100%) means the instrument
/// consistently closed higher than the prior bar over the lookback window.
///
/// ```text
/// above[t] = 1 if close[t] > close[t-1], else 0
/// ratio[t] = sum(above, period) / period x 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` comparisons have been made
/// (requires `period + 1` bars total).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAbovePrevClose;
/// use fin_primitives::signals::Signal;
/// let capc = CloseAbovePrevClose::new("capc", 10).unwrap();
/// assert_eq!(capc.period(), 10);
/// ```
pub struct CloseAbovePrevClose {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl CloseAbovePrevClose {
    /// Constructs a new `CloseAbovePrevClose`.
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

impl Signal for CloseAbovePrevClose {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let above: u8 = if bar.close > pc { 1 } else { 0 };
            self.window.push_back(above);
            self.count += above as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() { self.count -= old as usize; }
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let ratio = Decimal::from(self.count as u32)
            / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(ratio))
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
    fn test_capc_period_0_error() { assert!(CloseAbovePrevClose::new("capc", 0).is_err()); }

    #[test]
    fn test_capc_unavailable_before_period() {
        let mut capc = CloseAbovePrevClose::new("capc", 3).unwrap();
        // bar1: no prev, window=[] -> Unavailable
        assert_eq!(capc.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        // bar2: prev=100, 101>100, window=[1] -> Unavailable (1 < 3)
        assert_eq!(capc.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_capc_all_rising_is_100() {
        let mut capc = CloseAbovePrevClose::new("capc", 3).unwrap();
        capc.update_bar(&bar("100")).unwrap(); // no comparison yet
        capc.update_bar(&bar("101")).unwrap(); // 101>100 -> window=[1]
        capc.update_bar(&bar("102")).unwrap(); // 102>101 -> window=[1,1]
        let v = capc.update_bar(&bar("103")).unwrap(); // 103>102 -> window=[1,1,1] -> 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_capc_all_falling_is_0() {
        let mut capc = CloseAbovePrevClose::new("capc", 3).unwrap();
        capc.update_bar(&bar("103")).unwrap();
        capc.update_bar(&bar("102")).unwrap(); // not above -> window=[0]
        capc.update_bar(&bar("101")).unwrap(); // not above -> window=[0,0]
        let v = capc.update_bar(&bar("100")).unwrap(); // not above -> window=[0,0,0] -> 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_capc_window_slides() {
        let mut capc = CloseAbovePrevClose::new("capc", 2).unwrap();
        capc.update_bar(&bar("100")).unwrap(); // no comparison
        capc.update_bar(&bar("101")).unwrap(); // above -> window=[1]
        capc.update_bar(&bar("102")).unwrap(); // above -> window=[1,1] -> 100%
        let v = capc.update_bar(&bar("101")).unwrap(); // not above -> window=[1,0] -> 50%
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_capc_reset() {
        let mut capc = CloseAbovePrevClose::new("capc", 3).unwrap();
        for p in ["100", "101", "102", "103"] { capc.update_bar(&bar(p)).unwrap(); }
        assert!(capc.is_ready());
        capc.reset();
        assert!(!capc.is_ready());
    }
}
