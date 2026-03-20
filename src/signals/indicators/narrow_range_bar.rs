//! Narrow Range Bar — detects when the current bar has the smallest range in N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Narrow Range Bar (NR-N) — emits `1` when the current bar's range is the minimum
/// of the last `period` bars, `0` otherwise.
///
/// Narrow range bars signal potential volatility compression and breakout setups:
/// - **1**: current bar is the narrowest in the N-bar window (NR setup).
/// - **0**: not a narrow range bar.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NarrowRangeBar;
/// use fin_primitives::signals::Signal;
/// let nr = NarrowRangeBar::new("nr7", 7).unwrap();
/// assert_eq!(nr.period(), 7);
/// ```
pub struct NarrowRangeBar {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl NarrowRangeBar {
    /// Constructs a new `NarrowRangeBar`.
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
            ranges: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for NarrowRangeBar {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        self.ranges.push_back(range);
        if self.ranges.len() > self.period {
            self.ranges.pop_front();
        }
        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let is_narrowest = self.ranges
            .iter()
            .take(self.period - 1) // prior bars only
            .all(|&r| range <= r);

        let signal = if is_narrowest { Decimal::ONE } else { Decimal::ZERO };
        Ok(SignalValue::Scalar(signal))
    }

    fn reset(&mut self) {
        self.ranges.clear();
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
    fn test_nr_invalid_period() {
        assert!(NarrowRangeBar::new("nr", 0).is_err());
    }

    #[test]
    fn test_nr_unavailable_before_period() {
        let mut s = NarrowRangeBar::new("nr", 3).unwrap();
        assert_eq!(s.update_bar(&bar("110","90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("110","90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_nr_narrowest_bar_gives_one() {
        let mut s = NarrowRangeBar::new("nr", 3).unwrap();
        s.update_bar(&bar("120","80")).unwrap(); // range=40
        s.update_bar(&bar("115","85")).unwrap(); // range=30
        let v = s.update_bar(&bar("102","98")).unwrap(); // range=4 → narrowest → 1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_nr_wider_bar_gives_zero() {
        let mut s = NarrowRangeBar::new("nr", 3).unwrap();
        s.update_bar(&bar("102","98")).unwrap(); // range=4
        s.update_bar(&bar("105","95")).unwrap(); // range=10
        let v = s.update_bar(&bar("120","80")).unwrap(); // range=40 → not narrowest → 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_nr_period_1_always_narrowest() {
        let mut s = NarrowRangeBar::new("nr", 1).unwrap();
        // Period=1: window has only current bar → always narrowest
        let v = s.update_bar(&bar("120","80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_nr_reset() {
        let mut s = NarrowRangeBar::new("nr", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("110","90")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
