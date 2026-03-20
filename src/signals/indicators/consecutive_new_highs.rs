//! Consecutive New Highs indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Consecutive New Highs -- counts the current streak of bars making a new N-period high.
///
/// Each bar, the indicator checks whether the current `high` exceeds the rolling maximum
/// of the previous `period` bars. If it does, the streak counter increments; otherwise
/// it resets to zero.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (need a full window of prior bars before making the comparison).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ConsecutiveNewHighs;
/// use fin_primitives::signals::Signal;
/// let cnh = ConsecutiveNewHighs::new("cnh", 5).unwrap();
/// assert_eq!(cnh.period(), 5);
/// ```
pub struct ConsecutiveNewHighs {
    name: String,
    period: usize,
    window: VecDeque<Decimal>, // rolling window of prior highs
    streak: u32,
}

impl ConsecutiveNewHighs {
    /// Constructs a new `ConsecutiveNewHighs`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            streak: 0,
        })
    }
}

impl Signal for ConsecutiveNewHighs {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if self.window.len() < self.period {
            self.window.push_back(bar.high);
            return Ok(SignalValue::Unavailable);
        }

        // window is full: compare current high to max of window
        let prev_max = self.window.iter().copied().fold(Decimal::MIN, Decimal::max);
        if bar.high > prev_max {
            self.streak += 1;
        } else {
            self.streak = 0;
        }

        // slide window forward
        self.window.pop_front();
        self.window.push_back(bar.high);

        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.streak = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str) -> OhlcvBar {
        let p = Price::new(h.parse().unwrap()).unwrap();
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
    fn test_cnh_period_0_error() { assert!(ConsecutiveNewHighs::new("c", 0).is_err()); }

    #[test]
    fn test_cnh_unavailable_during_warmup() {
        let mut c = ConsecutiveNewHighs::new("c", 3).unwrap();
        assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        // 4th bar is first comparison
        let v = c.update_bar(&bar("101")).unwrap();
        assert!(v.is_scalar());
    }

    #[test]
    fn test_cnh_new_high_increments_streak() {
        let mut c = ConsecutiveNewHighs::new("c", 3).unwrap();
        // seed window with [100, 100, 100]
        for _ in 0..3 { c.update_bar(&bar("100")).unwrap(); }
        // 101 > max(100) -> streak=1
        assert_eq!(c.update_bar(&bar("101")).unwrap(), SignalValue::Scalar(dec!(1)));
        // 102 > max(100, 100, 101) = 101 -> streak=2
        assert_eq!(c.update_bar(&bar("102")).unwrap(), SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_cnh_no_new_high_resets_streak() {
        let mut c = ConsecutiveNewHighs::new("c", 3).unwrap();
        for _ in 0..3 { c.update_bar(&bar("100")).unwrap(); }
        c.update_bar(&bar("105")).unwrap(); // streak=1
        // 95 <= max of window (which contains 105 now) -> reset
        let v = c.update_bar(&bar("95")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cnh_reset() {
        let mut c = ConsecutiveNewHighs::new("c", 3).unwrap();
        for _ in 0..5 { c.update_bar(&bar("100")).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
        assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
