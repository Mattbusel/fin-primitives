//! Up Bar Ratio indicator -- fraction of up-bars (close > open) over last N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Up Bar Ratio -- percentage of bars where close > open over a rolling `period`-bar window.
///
/// Similar to [`crate::signals::indicators::CloseAboveOpen`] but with a clearer name
/// emphasizing it uses the open-to-close comparison within each bar.
///
/// ```text
/// up_bar[t]     = 1 if close > open, else 0
/// ratio[t]      = sum(up_bar, period) / period x 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpBarRatio;
/// use fin_primitives::signals::Signal;
/// let ubr = UpBarRatio::new("ubr", 10).unwrap();
/// assert_eq!(ubr.period(), 10);
/// ```
pub struct UpBarRatio {
    name: String,
    period: usize,
    window: VecDeque<u8>,
    count: usize,
}

impl UpBarRatio {
    /// Constructs a new `UpBarRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self { name: name.into(), period, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for UpBarRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let up: u8 = if bar.close > bar.open { 1 } else { 0 };
        self.window.push_back(up);
        self.count += up as usize;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.count -= old as usize; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let ratio = Decimal::from(self.count as u32)
            / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
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

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let high = if cp.value() > op.value() { cp } else { op };
        let low  = if cp.value() < op.value() { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high, low, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ubr_period_0_error() { assert!(UpBarRatio::new("u", 0).is_err()); }

    #[test]
    fn test_ubr_unavailable_before_period() {
        let mut u = UpBarRatio::new("u", 3).unwrap();
        assert_eq!(u.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ubr_all_up_is_100() {
        let mut u = UpBarRatio::new("u", 3).unwrap();
        for _ in 0..3 { u.update_bar(&bar("100", "105")).unwrap(); }
        let v = u.update_bar(&bar("100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_ubr_all_down_is_0() {
        let mut u = UpBarRatio::new("u", 3).unwrap();
        u.update_bar(&bar("105", "100")).unwrap();
        u.update_bar(&bar("105", "100")).unwrap();
        let v = u.update_bar(&bar("105", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ubr_half_half_is_50() {
        let mut u = UpBarRatio::new("u", 4).unwrap();
        u.update_bar(&bar("100", "105")).unwrap();
        u.update_bar(&bar("105", "100")).unwrap();
        u.update_bar(&bar("100", "105")).unwrap();
        let v = u.update_bar(&bar("105", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_ubr_reset() {
        let mut u = UpBarRatio::new("u", 2).unwrap();
        u.update_bar(&bar("100", "105")).unwrap();
        u.update_bar(&bar("100", "105")).unwrap();
        assert!(u.is_ready());
        u.reset();
        assert!(!u.is_ready());
    }
}
