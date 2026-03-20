//! Net High-Low Count indicator -- rolling higher-highs minus lower-lows count.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Net High-Low Count -- rolling count of higher-high bars minus lower-low bars.
///
/// Each bar is classified as:
/// - Higher high: `high[t] > high[t-1]` contributes +1
/// - Lower low:   `low[t]  < low[t-1]`  contributes -1
/// - A bar can be both (outside bar) contributing 0 net
///
/// ```text
/// hh[t] = 1 if high[t] > high[t-1], else 0
/// ll[t] = 1 if low[t]  < low[t-1],  else 0
/// net[t] = sum(hh - ll, period)
/// ```
///
/// Positive values indicate predominantly rising highs; negative values suggest
/// falling lows dominate.
///
/// Returns [`SignalValue::Unavailable`] until `period` comparisons have been made
/// (requires `period + 1` bars).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NetHighLowCount;
/// use fin_primitives::signals::Signal;
/// let nhl = NetHighLowCount::new("nhl", 10).unwrap();
/// assert_eq!(nhl.period(), 10);
/// ```
pub struct NetHighLowCount {
    name: String,
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    window: VecDeque<i8>,
    sum: i32,
}

impl NetHighLowCount {
    /// Constructs a new `NetHighLowCount`.
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
            sum: 0,
        })
    }
}

impl Signal for NetHighLowCount {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            let hh: i8 = if bar.high > ph { 1 } else { 0 };
            let ll: i8 = if bar.low < pl { 1 } else { 0 };
            let net: i8 = hh - ll;
            self.window.push_back(net);
            self.sum += net as i32;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() { self.sum -= old as i32; }
            }
        }
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(Decimal::from(self.sum)))
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
        self.window.clear();
        self.sum = 0;
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
        let mp = Price::new((hp.value() + lp.value()) / Decimal::TWO).unwrap();
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
    fn test_nhl_period_0_error() { assert!(NetHighLowCount::new("nhl", 0).is_err()); }

    #[test]
    fn test_nhl_unavailable_before_period() {
        let mut nhl = NetHighLowCount::new("nhl", 3).unwrap();
        assert_eq!(nhl.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_nhl_all_higher_highs() {
        // Rising highs, stable lows -> all higher highs, no lower lows -> net = +period
        let mut nhl = NetHighLowCount::new("nhl", 3).unwrap();
        nhl.update_bar(&bar("100", "90")).unwrap(); // prev
        nhl.update_bar(&bar("105", "90")).unwrap(); // hh=1, ll=0 -> net=+1
        nhl.update_bar(&bar("110", "90")).unwrap(); // hh=1, ll=0 -> net=+1
        let v = nhl.update_bar(&bar("115", "90")).unwrap(); // hh=1, ll=0 -> net=+1 -> sum=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_nhl_all_lower_lows() {
        // Stable highs, falling lows -> all lower lows -> net = -period
        let mut nhl = NetHighLowCount::new("nhl", 3).unwrap();
        nhl.update_bar(&bar("110", "95")).unwrap(); // prev
        nhl.update_bar(&bar("110", "90")).unwrap(); // hh=0, ll=1 -> net=-1
        nhl.update_bar(&bar("110", "85")).unwrap(); // hh=0, ll=1 -> net=-1
        let v = nhl.update_bar(&bar("110", "80")).unwrap(); // hh=0, ll=1 -> net=-1 -> sum=-3
        assert_eq!(v, SignalValue::Scalar(dec!(-3)));
    }

    #[test]
    fn test_nhl_flat_is_zero() {
        // Identical bars -> no HH, no LL -> net = 0
        let mut nhl = NetHighLowCount::new("nhl", 3).unwrap();
        nhl.update_bar(&bar("110", "90")).unwrap();
        nhl.update_bar(&bar("110", "90")).unwrap();
        nhl.update_bar(&bar("110", "90")).unwrap();
        let v = nhl.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_nhl_reset() {
        let mut nhl = NetHighLowCount::new("nhl", 2).unwrap();
        nhl.update_bar(&bar("110", "90")).unwrap();
        nhl.update_bar(&bar("115", "85")).unwrap();
        nhl.update_bar(&bar("120", "80")).unwrap();
        assert!(nhl.is_ready());
        nhl.reset();
        assert!(!nhl.is_ready());
    }
}
