//! Range Midpoint Position — close's position relative to the N-period high/low midpoint.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Midpoint Position — `(close - midpoint) / half_range` normalized to `[-1, 1]`.
///
/// Computes the N-period high/low midpoint and measures where the close sits:
/// - **+1.0**: close at the period high.
/// - **0.0**: close at the midpoint of the period range.
/// - **-1.0**: close at the period low.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated,
/// or when `high == low` (zero range).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeMidpointPosition;
/// use fin_primitives::signals::Signal;
/// let rmp = RangeMidpointPosition::new("rmp_20", 20).unwrap();
/// assert_eq!(rmp.period(), 20);
/// ```
pub struct RangeMidpointPosition {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    closes: VecDeque<Decimal>,
}

impl RangeMidpointPosition {
    /// Constructs a new `RangeMidpointPosition`.
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
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RangeMidpointPosition {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        self.closes.push_back(bar.close);

        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
            self.closes.pop_front();
        }

        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let period_high = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let period_low = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let close = *self.closes.back().unwrap();

        let range = period_high - period_low;
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let midpoint = (period_high + period_low)
            .checked_div(Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;
        let half_range = range
            .checked_div(Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;

        let pos = (close - midpoint)
            .checked_div(half_range)
            .ok_or(FinError::ArithmeticOverflow)?;

        // Clamp to [-1, 1]
        Ok(SignalValue::Scalar(pos.max(Decimal::NEGATIVE_ONE).min(Decimal::ONE)))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
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
    fn test_rmp_invalid_period() {
        assert!(RangeMidpointPosition::new("rmp", 0).is_err());
    }

    #[test]
    fn test_rmp_unavailable_before_period() {
        let mut s = RangeMidpointPosition::new("rmp", 3).unwrap();
        assert_eq!(s.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("112","88","100")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_rmp_close_at_midpoint_gives_zero() {
        // Period high=110, low=90 → midpoint=100. Close=100 → position=0
        let mut s = RangeMidpointPosition::new("rmp", 2).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110","90","100")).unwrap() {
            assert!(v.abs() < dec!(0.001), "close at midpoint should give 0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rmp_close_at_high_gives_positive() {
        let mut s = RangeMidpointPosition::new("rmp", 2).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110","90","110")).unwrap() {
            assert!(v > dec!(0), "close at high should give positive: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rmp_in_range_negative_one_to_one() {
        let mut s = RangeMidpointPosition::new("rmp", 3).unwrap();
        for (h,l,c) in &[("110","90","100"),("115","85","95"),("112","88","110"),("108","92","89")] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(h,l,c)).unwrap() {
                assert!(v >= dec!(-1) && v <= dec!(1), "position out of [-1,1]: {v}");
            }
        }
    }

    #[test]
    fn test_rmp_reset() {
        let mut s = RangeMidpointPosition::new("rmp", 2).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
