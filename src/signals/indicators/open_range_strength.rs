//! Open Range Strength — measures how far close extends beyond the open bar's range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Open Range Strength — `(close - open_range_high) / open_range_range` for breakouts,
/// smoothed as a rolling SMA over `period` bars.
///
/// For each bar, computes how much the close extends beyond (positive) or stays inside
/// (between -1 and 0) the `open_range_period` bar high-low range:
/// - Uses the first `open_range_period` bars to establish an initial range (high/low).
/// - Subsequent bars: `(close - open_range_high) / open_range_range` → positive = breakout up.
///
/// For simplicity, this uses a rolling approach: the "open range" is the high and low
/// of the current bar vs the prior bar, giving a 2-bar reference. The metric is:
/// `(close - max(curr_high, prev_high)) / abs_range`, clamped to a meaningful value.
///
/// Actually, a cleaner formulation: **Open Range Strength = (close - session_midpoint) / session_range**
/// where session_midpoint is the rolling N-bar midpoint. This produces a normalized close position
/// using an SMA-smoothed version of the range midpoint position.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenRangeStrength;
/// use fin_primitives::signals::Signal;
/// let ors = OpenRangeStrength::new("ors_5", 5).unwrap();
/// assert_eq!(ors.period(), 5);
/// ```
pub struct OpenRangeStrength {
    name: String,
    period: usize,
    // Rolling EMA of (close - mid) / half_range where mid/range are rolling N-bar HL stats
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    closes: VecDeque<Decimal>,
}

impl OpenRangeStrength {
    /// Constructs a new `OpenRangeStrength`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
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

impl Signal for OpenRangeStrength {
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
        let range = period_high - period_low;

        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let mid = (period_high + period_low)
            .checked_div(Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;

        let close = *self.closes.back().unwrap();
        let strength = (close - mid)
            .checked_div(range
                .checked_div(Decimal::TWO)
                .ok_or(FinError::ArithmeticOverflow)?)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(strength.max(Decimal::NEGATIVE_ONE).min(Decimal::ONE)))
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
    fn test_ors_invalid_period() {
        assert!(OpenRangeStrength::new("ors", 0).is_err());
        assert!(OpenRangeStrength::new("ors", 1).is_err());
    }

    #[test]
    fn test_ors_unavailable_before_period() {
        let mut s = OpenRangeStrength::new("ors", 3).unwrap();
        assert_eq!(s.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_ors_close_at_high_gives_one() {
        // period=2, H=110, L=90 → mid=100, range=20, half=10
        // Close at 110 → (110-100)/10 = 1.0
        let mut s = OpenRangeStrength::new("ors", 2).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110","90","110")).unwrap() {
            assert!((v - dec!(1)).abs() < dec!(0.001), "close at high gives 1.0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ors_close_at_midpoint_gives_zero() {
        let mut s = OpenRangeStrength::new("ors", 2).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110","90","100")).unwrap() {
            assert!(v.abs() < dec!(0.001), "close at midpoint gives 0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ors_in_range_neg_one_to_one() {
        let mut s = OpenRangeStrength::new("ors", 3).unwrap();
        for (h,l,c) in &[("110","90","100"),("115","85","95"),("112","88","110"),("108","92","89")] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(h,l,c)).unwrap() {
                assert!(v >= dec!(-1) && v <= dec!(1), "value out of [-1,1]: {v}");
            }
        }
    }

    #[test]
    fn test_ors_reset() {
        let mut s = OpenRangeStrength::new("ors", 2).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        s.update_bar(&bar("110","90","100")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
