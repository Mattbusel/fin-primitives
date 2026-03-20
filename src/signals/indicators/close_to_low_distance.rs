//! Close-to-Low Distance indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close-to-Low Distance — how far the current close is above the rolling
/// minimum low over `period` bars, expressed as a multiple of ATR.
///
/// ```text
/// distance = (close - min_low(n)) / ATR(n)
/// ```
///
/// A value of 0 means the close equals the rolling low. A large positive value
/// indicates the close is well above the floor.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or ATR is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseToLowDistance;
/// use fin_primitives::signals::Signal;
///
/// let ctld = CloseToLowDistance::new("ctld", 14).unwrap();
/// assert_eq!(ctld.period(), 14);
/// ```
pub struct CloseToLowDistance {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    lows: VecDeque<Decimal>,
    trs: VecDeque<Decimal>,
    tr_sum: Decimal,
}

impl CloseToLowDistance {
    /// Constructs a new `CloseToLowDistance`.
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
            prev_close: None,
            lows: VecDeque::with_capacity(period),
            trs: VecDeque::with_capacity(period),
            tr_sum: Decimal::ZERO,
        })
    }
}

impl Signal for CloseToLowDistance {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.lows.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = bar.true_range(self.prev_close);
        self.prev_close = Some(bar.close);

        self.trs.push_back(tr);
        self.tr_sum += tr;
        if self.trs.len() > self.period {
            self.tr_sum -= self.trs.pop_front().unwrap();
        }

        self.lows.push_back(bar.low);
        if self.lows.len() > self.period { self.lows.pop_front(); }

        if self.lows.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let nd = Decimal::from(self.period as u32);
        let atr = self.tr_sum / nd;
        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let min_low = self.lows.iter().copied().fold(self.lows[0], |acc, v| acc.min(v));
        Ok(SignalValue::Scalar((bar.close - min_low) / atr))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.lows.clear();
        self.trs.clear();
        self.tr_sum = Decimal::ZERO;
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
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ctld_invalid_period() {
        assert!(CloseToLowDistance::new("ctld", 0).is_err());
    }

    #[test]
    fn test_ctld_unavailable_before_warm_up() {
        let mut ctld = CloseToLowDistance::new("ctld", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(ctld.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ctld_close_at_low_gives_zero() {
        let mut ctld = CloseToLowDistance::new("ctld", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        // All bars: low=100, close=100, high=110 → min_low=100, close=100 → distance=0
        for _ in 0..3 {
            last = ctld.update_bar(&bar("110", "100", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ctld_close_above_low_positive() {
        let mut ctld = CloseToLowDistance::new("ctld", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = ctld.update_bar(&bar("110", "90", "105")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "close above min low should give positive distance: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ctld_reset() {
        let mut ctld = CloseToLowDistance::new("ctld", 3).unwrap();
        for _ in 0..3 { ctld.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(ctld.is_ready());
        ctld.reset();
        assert!(!ctld.is_ready());
    }
}
