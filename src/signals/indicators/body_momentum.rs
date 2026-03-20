//! Body Momentum — rolling sum of bar body moves (close - open).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body Momentum — `sum(close - open)` over the last `period` bars.
///
/// Accumulates the signed intrabar body moves:
/// - **Positive**: net bullish body displacement — more bullish than bearish bars.
/// - **Negative**: net bearish.
/// - **0**: balanced.
///
/// Unlike price momentum (close-to-close), this only counts intrabar movement, ignoring
/// overnight gaps. Useful for measuring session-level directional force.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyMomentum;
/// use fin_primitives::signals::Signal;
/// let bm = BodyMomentum::new("bm_10", 10).unwrap();
/// assert_eq!(bm.period(), 10);
/// ```
pub struct BodyMomentum {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl BodyMomentum {
    /// Constructs a new `BodyMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for BodyMomentum {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = bar.net_move();
        self.sum += body;
        self.window.push_back(body);
        if self.window.len() > self.period {
            let removed = self.window.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum))
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

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp.max(op), low: cp.min(op), close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bm_invalid_period() {
        assert!(BodyMomentum::new("bm", 0).is_err());
    }

    #[test]
    fn test_bm_unavailable_before_period() {
        let mut s = BodyMomentum::new("bm", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100","105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("105","110")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bm_all_bullish() {
        let mut s = BodyMomentum::new("bm", 3).unwrap();
        // bodies: 5, 5, 5 → sum=15
        s.update_bar(&bar("100","105")).unwrap();
        s.update_bar(&bar("105","110")).unwrap();
        let v = s.update_bar(&bar("110","115")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(15)));
    }

    #[test]
    fn test_bm_mixed_bars_sum() {
        let mut s = BodyMomentum::new("bm", 3).unwrap();
        // bodies: +5, -3, +2 → sum=4
        s.update_bar(&bar("100","105")).unwrap();
        s.update_bar(&bar("105","102")).unwrap();
        let v = s.update_bar(&bar("102","104")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(4)));
    }

    #[test]
    fn test_bm_doji_zero() {
        let mut s = BodyMomentum::new("bm", 2).unwrap();
        s.update_bar(&bar("100","100")).unwrap();
        let v = s.update_bar(&bar("100","100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bm_reset() {
        let mut s = BodyMomentum::new("bm", 2).unwrap();
        s.update_bar(&bar("100","105")).unwrap();
        s.update_bar(&bar("105","110")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
