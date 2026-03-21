//! Return Concentration indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Return Concentration.
///
/// Measures what fraction of the total `period`-bar return happened in the most
/// recent `recent_bars` bars. High concentration suggests momentum is accelerating;
/// low concentration suggests it is distributing/decelerating.
///
/// Formula:
/// `concentration = recent_return / total_return`
///
/// Where:
/// - `recent_return = close_t / close_{t - recent_bars} - 1`
/// - `total_return = close_t / close_{t - period} - 1`
///
/// Returns `0.0` when `total_return == 0`.
/// Returns `SignalValue::Unavailable` until `period + 1` closes are seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ReturnConcentration;
/// use fin_primitives::signals::Signal;
/// let rc = ReturnConcentration::new("rc_20_5", 20, 5).unwrap();
/// assert_eq!(rc.period(), 20);
/// ```
pub struct ReturnConcentration {
    name: String,
    period: usize,
    recent_bars: usize,
    closes: VecDeque<Decimal>,
}

impl ReturnConcentration {
    /// Constructs a new `ReturnConcentration`.
    ///
    /// - `period`: total lookback window.
    /// - `recent_bars`: how many of the most recent bars to measure (must be < period).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0` or `recent_bars >= period`.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        recent_bars: usize,
    ) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        if recent_bars >= period {
            return Err(FinError::InvalidPeriod(recent_bars));
        }
        Ok(Self {
            name: name.into(),
            period,
            recent_bars,
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for ReturnConcentration {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let current = *self.closes.back().unwrap();
        let total_base = *self.closes.front().unwrap();  // period bars ago
        let recent_base_idx = self.period - self.recent_bars; // index of recent_base
        let recent_base = self.closes[recent_base_idx];

        if total_base.is_zero() || recent_base.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let total_return = (current - total_base)
            .checked_div(total_base)
            .ok_or(FinError::ArithmeticOverflow)?;
        if total_return.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let recent_return = (current - recent_base)
            .checked_div(recent_base)
            .ok_or(FinError::ArithmeticOverflow)?;
        let concentration = recent_return
            .checked_div(total_return)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(concentration))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period + 1
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_invalid_params() {
        assert!(ReturnConcentration::new("rc", 0, 0).is_err());
        assert!(ReturnConcentration::new("rc", 5, 5).is_err());
        assert!(ReturnConcentration::new("rc", 5, 6).is_err());
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut rc = ReturnConcentration::new("rc", 5, 2).unwrap();
        let v = rc.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_ready_after_period_plus_one() {
        let mut rc = ReturnConcentration::new("rc", 3, 1).unwrap();
        for _ in 0..4 {
            rc.update_bar(&bar("100")).unwrap();
        }
        assert!(rc.is_ready());
    }

    #[test]
    fn test_flat_returns_zero() {
        let mut rc = ReturnConcentration::new("rc", 3, 1).unwrap();
        for _ in 0..4 {
            rc.update_bar(&bar("100")).unwrap();
        }
        let v = rc.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut rc = ReturnConcentration::new("rc", 3, 1).unwrap();
        for _ in 0..4 {
            rc.update_bar(&bar("100")).unwrap();
        }
        assert!(rc.is_ready());
        rc.reset();
        assert!(!rc.is_ready());
    }
}
