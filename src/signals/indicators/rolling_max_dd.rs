//! Rolling Maximum Drawdown indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Maximum Drawdown -- the largest peak-to-trough decline within the
/// rolling `period`-bar window.
///
/// ```text
/// peak_so_far[t]    = max(close, period)
/// dd[t]             = (close[t] - peak[t]) / peak[t] * 100   (always <= 0)
/// max_dd[t]         = min of dd values over the period
/// ```
///
/// Returns a negative percentage representing the worst drawdown.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingMaxDd;
/// use fin_primitives::signals::Signal;
/// let rmdd = RollingMaxDd::new("rmdd", 20).unwrap();
/// assert_eq!(rmdd.period(), 20);
/// ```
pub struct RollingMaxDd {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl RollingMaxDd {
    /// Constructs a new `RollingMaxDd`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RollingMaxDd {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        // Compute peak-to-trough drawdown over the window
        let mut max_dd = Decimal::ZERO;
        let mut peak = Decimal::MIN;
        for &price in &self.window {
            if price > peak { peak = price; }
            if peak.is_zero() { continue; }
            let dd = (price - peak) / peak * Decimal::ONE_HUNDRED;
            if dd < max_dd { max_dd = dd; }
        }
        Ok(SignalValue::Scalar(max_dd))
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
    fn test_rmdd_period_0_error() { assert!(RollingMaxDd::new("rmdd", 0).is_err()); }

    #[test]
    fn test_rmdd_unavailable_before_period() {
        let mut rmdd = RollingMaxDd::new("rmdd", 3).unwrap();
        assert_eq!(rmdd.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rmdd_monotone_rising_no_drawdown() {
        // Rising prices -> no drawdown (peak always == close) -> max_dd = 0
        let mut rmdd = RollingMaxDd::new("rmdd", 3).unwrap();
        rmdd.update_bar(&bar("100")).unwrap();
        rmdd.update_bar(&bar("110")).unwrap();
        let v = rmdd.update_bar(&bar("120")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rmdd_peak_then_trough() {
        // 100 -> 200 -> 100: dd = (100-200)/200*100 = -50%
        let mut rmdd = RollingMaxDd::new("rmdd", 3).unwrap();
        rmdd.update_bar(&bar("100")).unwrap();
        rmdd.update_bar(&bar("200")).unwrap();
        let v = rmdd.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-50)));
    }

    #[test]
    fn test_rmdd_window_slides_out_peak() {
        // Once the peak bar slides out of the window, drawdown recovers
        let mut rmdd = RollingMaxDd::new("rmdd", 3).unwrap();
        rmdd.update_bar(&bar("200")).unwrap(); // peak
        rmdd.update_bar(&bar("100")).unwrap(); // -50%
        rmdd.update_bar(&bar("110")).unwrap(); // -45%
        // Now slides: window=[100, 110, 120], peak=120, no drawdown
        let v = rmdd.update_bar(&bar("120")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rmdd_reset() {
        let mut rmdd = RollingMaxDd::new("rmdd", 3).unwrap();
        for p in ["100", "200", "100"] { rmdd.update_bar(&bar(p)).unwrap(); }
        assert!(rmdd.is_ready());
        rmdd.reset();
        assert!(!rmdd.is_ready());
    }
}
