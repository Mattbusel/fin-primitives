//! Close Above Midpoint indicator -- rolling % of bars where close > bar midpoint.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Above Midpoint -- percentage of bars in the last `period` where the close
/// is above the bar's own midpoint `(high + low) / 2`.
///
/// When close is above the midpoint, buyers dominated that bar. Values near 100%
/// indicate consistent bullish closes; near 0% indicates persistent bearish closes.
///
/// ```text
/// mid[t]       = (high + low) / 2
/// above[t]     = 1 if close[t] > mid[t], else 0
/// ratio[t]     = sum(above, period) / period x 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAboveMidpoint;
/// use fin_primitives::signals::Signal;
/// let cam = CloseAboveMidpoint::new("cam", 10).unwrap();
/// assert_eq!(cam.period(), 10);
/// ```
pub struct CloseAboveMidpoint {
    name: String,
    period: usize,
    window: VecDeque<u8>,
    count: usize,
}

impl CloseAboveMidpoint {
    /// Constructs a new `CloseAboveMidpoint`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for CloseAboveMidpoint {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = (bar.high + bar.low) / Decimal::TWO;
        let above: u8 = if bar.close > mid { 1 } else { 0 };
        self.window.push_back(above);
        self.count += above as usize;
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cam_period_0_error() { assert!(CloseAboveMidpoint::new("c", 0).is_err()); }

    #[test]
    fn test_cam_unavailable_before_period() {
        let mut c = CloseAboveMidpoint::new("c", 3).unwrap();
        assert_eq!(c.update_bar(&bar("110", "90", "108")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cam_all_above_is_100() {
        let mut c = CloseAboveMidpoint::new("c", 3).unwrap();
        // mid = 100, close = 108 > 100 on all bars
        c.update_bar(&bar("110", "90", "108")).unwrap();
        c.update_bar(&bar("110", "90", "108")).unwrap();
        let v = c.update_bar(&bar("110", "90", "108")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_cam_all_below_is_0() {
        let mut c = CloseAboveMidpoint::new("c", 3).unwrap();
        // mid = 100, close = 92 < 100
        c.update_bar(&bar("110", "90", "92")).unwrap();
        c.update_bar(&bar("110", "90", "92")).unwrap();
        let v = c.update_bar(&bar("110", "90", "92")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cam_reset() {
        let mut c = CloseAboveMidpoint::new("c", 2).unwrap();
        c.update_bar(&bar("110", "90", "108")).unwrap();
        c.update_bar(&bar("110", "90", "108")).unwrap();
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
