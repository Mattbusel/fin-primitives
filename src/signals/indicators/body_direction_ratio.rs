//! Body Direction Ratio indicator -- rolling ratio of bullish to bearish candle bodies.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body Direction Ratio -- rolling ratio of total bullish body size to total bearish
/// body size over `period` bars.
///
/// ```text
/// body[t]      = |close - open|
/// bull_body[t] = body[t] if close > open, else 0
/// bear_body[t] = body[t] if close < open, else 0
/// ratio[t]     = sum(bull_body, period) / sum(bear_body, period)
/// ```
///
/// Values > 1 indicate bulls have more total body size (buying momentum dominates).
/// Values < 1 indicate bears dominate. Returns 0 if no bearish bodies exist in window.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// total bearish body size is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyDirectionRatio;
/// use fin_primitives::signals::Signal;
/// let bdr = BodyDirectionRatio::new("bdr", 10).unwrap();
/// assert_eq!(bdr.period(), 10);
/// ```
pub struct BodyDirectionRatio {
    name: String,
    period: usize,
    bull_window: VecDeque<Decimal>,
    bear_window: VecDeque<Decimal>,
    bull_sum: Decimal,
    bear_sum: Decimal,
}

impl BodyDirectionRatio {
    /// Constructs a new `BodyDirectionRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            bull_window: VecDeque::with_capacity(period),
            bear_window: VecDeque::with_capacity(period),
            bull_sum: Decimal::ZERO,
            bear_sum: Decimal::ZERO,
        })
    }
}

impl Signal for BodyDirectionRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.bull_window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = (bar.net_move()).abs();
        let bull = if bar.is_bullish() { body } else { Decimal::ZERO };
        let bear = if bar.is_bearish() { body } else { Decimal::ZERO };
        self.bull_window.push_back(bull);
        self.bear_window.push_back(bear);
        self.bull_sum += bull;
        self.bear_sum += bear;
        if self.bull_window.len() > self.period {
            if let Some(old_b) = self.bull_window.pop_front() { self.bull_sum -= old_b; }
            if let Some(old_r) = self.bear_window.pop_front() { self.bear_sum -= old_r; }
        }
        if self.bull_window.len() < self.period { return Ok(SignalValue::Unavailable); }
        if self.bear_sum.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(self.bull_sum / self.bear_sum))
    }

    fn reset(&mut self) {
        self.bull_window.clear();
        self.bear_window.clear();
        self.bull_sum = Decimal::ZERO;
        self.bear_sum = Decimal::ZERO;
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
    fn test_bdr_period_0_error() { assert!(BodyDirectionRatio::new("bdr", 0).is_err()); }

    #[test]
    fn test_bdr_unavailable_before_period() {
        let mut bdr = BodyDirectionRatio::new("bdr", 3).unwrap();
        assert_eq!(bdr.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bdr_all_bullish_unavailable() {
        // No bearish bodies -> bear_sum=0 -> Unavailable
        let mut bdr = BodyDirectionRatio::new("bdr", 3).unwrap();
        bdr.update_bar(&bar("100", "105")).unwrap();
        bdr.update_bar(&bar("105", "110")).unwrap();
        let v = bdr.update_bar(&bar("110", "115")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_bdr_equal_bodies_is_1() {
        // Equal bull and bear bodies -> ratio = 1
        let mut bdr = BodyDirectionRatio::new("bdr", 2).unwrap();
        bdr.update_bar(&bar("100", "110")).unwrap(); // bull body = 10
        let v = bdr.update_bar(&bar("110", "100")).unwrap(); // bear body = 10, ratio=10/10=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bdr_bull_dominates() {
        // Bull body 20, bear body 10 -> ratio = 2
        let mut bdr = BodyDirectionRatio::new("bdr", 2).unwrap();
        bdr.update_bar(&bar("100", "120")).unwrap(); // bull=20
        let v = bdr.update_bar(&bar("120", "110")).unwrap(); // bear=10, ratio=20/10=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_bdr_reset() {
        let mut bdr = BodyDirectionRatio::new("bdr", 2).unwrap();
        bdr.update_bar(&bar("100", "110")).unwrap();
        bdr.update_bar(&bar("110", "100")).unwrap();
        assert!(bdr.is_ready());
        bdr.reset();
        assert!(!bdr.is_ready());
    }
}
