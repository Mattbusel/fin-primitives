//! Outside Bar Count indicator -- rolling count of outside bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Outside Bar Count -- rolling count of outside bars over the last `period` comparisons.
///
/// An outside bar has `high > prev_high AND low < prev_low`, meaning it completely
/// engulfs the previous bar's range. Such bars often signal volatility expansion
/// or indecision before a significant directional move.
///
/// ```text
/// outside[t] = 1 if high[t] > high[t-1] AND low[t] < low[t-1], else 0
/// count[t]   = sum(outside, period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated
/// (the first bar has no prior bar to compare against, so contributes 0).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OutsideBarCount;
/// use fin_primitives::signals::Signal;
/// let obc = OutsideBarCount::new("obc", 10).unwrap();
/// assert_eq!(obc.period(), 10);
/// ```
pub struct OutsideBarCount {
    name: String,
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl OutsideBarCount {
    /// Constructs a new `OutsideBarCount`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_high: None,
            prev_low: None,
            window: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for OutsideBarCount {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let outside: u8 = match (self.prev_high, self.prev_low) {
            (Some(ph), Some(pl)) if bar.high > ph && bar.low < pl => 1,
            _ => 0,
        };
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        self.window.push_back(outside);
        self.count += outside as usize;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.count -= old as usize; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let mid_v = (hp.value() + lp.value()) / Decimal::TWO;
        let mp = Price::new(mid_v).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: mp, high: hp, low: lp, close: mp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_obc_period_0_error() { assert!(OutsideBarCount::new("obc", 0).is_err()); }

    #[test]
    fn test_obc_unavailable_before_period() {
        let mut obc = OutsideBarCount::new("obc", 3).unwrap();
        assert_eq!(obc.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_obc_no_outside_bars() {
        let mut obc = OutsideBarCount::new("obc", 3).unwrap();
        // identical bars: each bar's high == prev high → not outside
        obc.update_bar(&bar("110", "90")).unwrap();
        obc.update_bar(&bar("110", "90")).unwrap();
        obc.update_bar(&bar("110", "90")).unwrap();
        let v = obc.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_obc_all_outside_bars() {
        let mut obc = OutsideBarCount::new("obc", 3).unwrap();
        obc.update_bar(&bar("105", "95")).unwrap(); // first bar (no prev)
        obc.update_bar(&bar("110", "90")).unwrap(); // outside: 110>105, 90<95
        obc.update_bar(&bar("115", "85")).unwrap(); // outside: 115>110, 85<90
        let v = obc.update_bar(&bar("120", "80")).unwrap(); // outside: 120>115, 80<85
        // window=[1,1,1], count=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_obc_one_outside_in_window() {
        let mut obc = OutsideBarCount::new("obc", 3).unwrap();
        obc.update_bar(&bar("110", "90")).unwrap(); // first bar
        obc.update_bar(&bar("115", "85")).unwrap(); // outside ✓
        obc.update_bar(&bar("112", "88")).unwrap(); // not outside (112 < 115)
        let v = obc.update_bar(&bar("111", "89")).unwrap(); // not outside (111 < 112)
        // window after slide: [1, 0, 0], count=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_obc_reset() {
        let mut obc = OutsideBarCount::new("obc", 2).unwrap();
        obc.update_bar(&bar("110", "90")).unwrap();
        obc.update_bar(&bar("115", "85")).unwrap();
        obc.update_bar(&bar("120", "80")).unwrap();
        assert!(obc.is_ready());
        obc.reset();
        assert!(!obc.is_ready());
    }
}
