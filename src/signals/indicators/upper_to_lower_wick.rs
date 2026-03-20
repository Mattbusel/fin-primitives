//! Upper to Lower Wick Ratio indicator -- rolling mean of upper/lower wick ratio.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Upper to Lower Wick Ratio -- rolling average of `upper_wick / lower_wick` per bar.
///
/// Upper wick = `high - max(open, close)`.
/// Lower wick = `min(open, close) - low`.
///
/// Values > 1 indicate upper wicks dominate (bearish rejection), suggesting selling
/// pressure at highs. Values < 1 indicate lower wicks dominate (bullish rejection).
///
/// Bars with a zero lower wick are excluded from the rolling average.
///
/// Returns [`SignalValue::Unavailable`] until `period` valid bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpperToLowerWick;
/// use fin_primitives::signals::Signal;
/// let utlw = UpperToLowerWick::new("utlw", 14).unwrap();
/// assert_eq!(utlw.period(), 14);
/// ```
pub struct UpperToLowerWick {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl UpperToLowerWick {
    /// Constructs a new `UpperToLowerWick`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for UpperToLowerWick {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body_top = bar.open.max(bar.close);
        let body_bot = bar.open.min(bar.close);
        let upper_wick = bar.high - body_top;
        let lower_wick = body_bot - bar.low;
        if lower_wick.is_zero() { return Ok(SignalValue::Unavailable); }
        let ratio = upper_wick / lower_wick;
        self.window.push_back(ratio);
        self.sum += ratio;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(self.sum / Decimal::from(self.period as u32)))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_utlw_period_0_error() { assert!(UpperToLowerWick::new("utlw", 0).is_err()); }

    #[test]
    fn test_utlw_zero_lower_wick_unavailable() {
        // open=100, high=110, low=100, close=105: lower_wick = min(100,105) - 100 = 0
        let mut utlw = UpperToLowerWick::new("utlw", 1).unwrap();
        let v = utlw.update_bar(&bar("100", "110", "100", "105")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_utlw_equal_wicks() {
        // open=100, high=110, low=90, close=100: upper=(110-100)=10, lower=(100-90)=10, ratio=1
        let mut utlw = UpperToLowerWick::new("utlw", 1).unwrap();
        let v = utlw.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_utlw_double_upper_wick() {
        // open=100, high=120, low=90, close=100: upper=20, lower=10, ratio=2
        let mut utlw = UpperToLowerWick::new("utlw", 1).unwrap();
        let v = utlw.update_bar(&bar("100", "120", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_utlw_rolling_average() {
        // Two bars with equal wicks (ratio=1 each) -> avg=1
        let mut utlw = UpperToLowerWick::new("utlw", 2).unwrap();
        utlw.update_bar(&bar("100", "110", "90", "100")).unwrap(); // ratio=1
        let v = utlw.update_bar(&bar("100", "110", "90", "100")).unwrap(); // ratio=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_utlw_reset() {
        let mut utlw = UpperToLowerWick::new("utlw", 2).unwrap();
        utlw.update_bar(&bar("100", "110", "90", "100")).unwrap();
        utlw.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert!(utlw.is_ready());
        utlw.reset();
        assert!(!utlw.is_ready());
    }
}
