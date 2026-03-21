//! Close to Midrange indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close to Midrange.
///
/// Measures where the current close sits relative to the rolling period high-low midpoint.
/// Expressed as a percentage of the half-range:
///
/// Formula:
/// - `period_high = max(high, period)`
/// - `period_low = min(low, period)`
/// - `midpoint = (period_high + period_low) / 2`
/// - `half_range = (period_high - period_low) / 2`
/// - `ctm = (close - midpoint) / half_range * 100`
///
/// - +100: close is at the period high.
/// - −100: close is at the period low.
/// - 0: close is exactly at the midpoint.
/// - Returns 0 when half_range is zero (all prices equal).
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseToMidrange;
/// use fin_primitives::signals::Signal;
/// let ctm = CloseToMidrange::new("ctm_20", 20).unwrap();
/// assert_eq!(ctm.period(), 20);
/// ```
pub struct CloseToMidrange {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    last_close: Decimal,
}

impl CloseToMidrange {
    /// Constructs a new `CloseToMidrange`.
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
            last_close: Decimal::ZERO,
        })
    }
}

impl Signal for CloseToMidrange {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        self.last_close = bar.close;

        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let period_high = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let period_low = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let half_range = (period_high - period_low)
            .checked_div(Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;

        if half_range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let midpoint = (period_high + period_low)
            .checked_div(Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;

        let ctm = (self.last_close - midpoint)
            .checked_div(half_range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ctm))
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.last_close = Decimal::ZERO;
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
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(CloseToMidrange::new("ctm", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut ctm = CloseToMidrange::new("ctm", 3).unwrap();
        assert_eq!(ctm.update_bar(&bar("12", "10", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_close_at_high_is_100() {
        let mut ctm = CloseToMidrange::new("ctm", 3).unwrap();
        for _ in 0..3 {
            ctm.update_bar(&bar("110", "90", "100")).unwrap();
        }
        // Close at period_high=110, mid=100, half_range=10 → (110-100)/10*100=100
        let v = ctm.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_close_at_midpoint_is_zero() {
        let mut ctm = CloseToMidrange::new("ctm", 3).unwrap();
        for _ in 0..3 {
            ctm.update_bar(&bar("110", "90", "100")).unwrap();
        }
        let v = ctm.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut ctm = CloseToMidrange::new("ctm", 2).unwrap();
        ctm.update_bar(&bar("12", "10", "11")).unwrap();
        ctm.update_bar(&bar("12", "10", "11")).unwrap();
        assert!(ctm.is_ready());
        ctm.reset();
        assert!(!ctm.is_ready());
    }
}
