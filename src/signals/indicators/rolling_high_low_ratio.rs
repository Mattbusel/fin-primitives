//! Rolling High/Low Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling High/Low Ratio — the ratio of the rolling maximum high to the
/// rolling minimum low over `period` bars.
///
/// ```text
/// ratio = max_high(n) / min_low(n)
/// ```
///
/// A ratio near 1.0 indicates a tight, compressed range. A high ratio indicates
/// a wide price range relative to the floor.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or min low is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingHighLowRatio;
/// use fin_primitives::signals::Signal;
///
/// let rhlr = RollingHighLowRatio::new("rhlr", 20).unwrap();
/// assert_eq!(rhlr.period(), 20);
/// ```
pub struct RollingHighLowRatio {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl RollingHighLowRatio {
    /// Constructs a new `RollingHighLowRatio`.
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
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RollingHighLowRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.highs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period { self.highs.pop_front(); }
        if self.lows.len() > self.period { self.lows.pop_front(); }

        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let max_h = self.highs.iter().copied().fold(self.highs[0], |acc, v| acc.max(v));
        let min_l = self.lows.iter().copied().fold(self.lows[0], |acc, v| acc.min(v));

        if min_l.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(max_h / min_l))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
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
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rhlr_invalid_period() {
        assert!(RollingHighLowRatio::new("rhlr", 0).is_err());
    }

    #[test]
    fn test_rhlr_unavailable_before_warm_up() {
        let mut rhlr = RollingHighLowRatio::new("rhlr", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(rhlr.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rhlr_constant_gives_ratio() {
        // max_h=110, min_l=90 → 110/90
        let mut rhlr = RollingHighLowRatio::new("rhlr", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = rhlr.update_bar(&bar("110", "90")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(1), "ratio should be > 1 when high > low: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rhlr_same_high_low_gives_one() {
        // max_h = min_l → ratio=1
        let mut rhlr = RollingHighLowRatio::new("rhlr", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = rhlr.update_bar(&bar("100", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_rhlr_reset() {
        let mut rhlr = RollingHighLowRatio::new("rhlr", 3).unwrap();
        for _ in 0..3 { rhlr.update_bar(&bar("110", "90")).unwrap(); }
        assert!(rhlr.is_ready());
        rhlr.reset();
        assert!(!rhlr.is_ready());
    }
}
