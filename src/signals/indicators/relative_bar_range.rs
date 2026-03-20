//! Relative Bar Range — current bar's range as a multiple of the N-period average range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Relative Bar Range — `(high - low) / average_range(period)`.
///
/// Compares the current bar's range to its rolling average:
/// - **> 1.0**: current bar is wider than average (above-average volatility).
/// - **= 1.0**: exactly average.
/// - **< 1.0**: current bar is narrower than average (below-average volatility).
///
/// Unlike ATR, this uses `high - low` (no gap adjustment), making it suitable
/// for intraday sessions where overnight gaps are irrelevant.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or
/// when the average range is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RelativeBarRange;
/// use fin_primitives::signals::Signal;
/// let rbr = RelativeBarRange::new("rbr_14", 14).unwrap();
/// assert_eq!(rbr.period(), 14);
/// ```
pub struct RelativeBarRange {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
    sum: Decimal,
}

impl RelativeBarRange {
    /// Constructs a new `RelativeBarRange`.
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
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RelativeBarRange {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.ranges.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        self.sum += range;
        self.ranges.push_back(range);
        if self.ranges.len() > self.period {
            let removed = self.ranges.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg_range = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if avg_range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = range.checked_div(avg_range).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.ranges.clear();
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
    fn test_rbr_invalid_period() {
        assert!(RelativeBarRange::new("rbr", 0).is_err());
    }

    #[test]
    fn test_rbr_unavailable_before_period() {
        let mut rbr = RelativeBarRange::new("rbr", 3).unwrap();
        assert_eq!(rbr.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(rbr.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!rbr.is_ready());
    }

    #[test]
    fn test_rbr_constant_range_gives_one() {
        // All bars same range → current range = average → ratio = 1.0
        let mut rbr = RelativeBarRange::new("rbr", 3).unwrap();
        for _ in 0..3 {
            rbr.update_bar(&bar("110", "90")).unwrap();
        }
        let v = rbr.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_rbr_wide_bar_above_one() {
        let mut rbr = RelativeBarRange::new("rbr", 3).unwrap();
        // Seed with narrow bars (range=5)
        rbr.update_bar(&bar("105", "100")).unwrap();
        rbr.update_bar(&bar("105", "100")).unwrap();
        rbr.update_bar(&bar("105", "100")).unwrap();
        // Very wide bar (range=50): 50/5 = 10
        let v = rbr.update_bar(&bar("150", "100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(1), "wide bar should give ratio > 1: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rbr_narrow_bar_below_one() {
        let mut rbr = RelativeBarRange::new("rbr", 3).unwrap();
        // Seed with wide bars (range=20)
        rbr.update_bar(&bar("110", "90")).unwrap();
        rbr.update_bar(&bar("110", "90")).unwrap();
        rbr.update_bar(&bar("110", "90")).unwrap();
        // Narrow bar (range=2): 2/20 = 0.1 (though window now includes the 2)
        let v = rbr.update_bar(&bar("101", "99")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r < dec!(1), "narrow bar should give ratio < 1: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rbr_non_negative() {
        let mut rbr = RelativeBarRange::new("rbr", 5).unwrap();
        let bars = [
            bar("110", "90"), bar("108", "92"), bar("115", "85"),
            bar("102", "98"), bar("112", "88"), bar("107", "93"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = rbr.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "relative range must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_rbr_reset() {
        let mut rbr = RelativeBarRange::new("rbr", 3).unwrap();
        for _ in 0..4 {
            rbr.update_bar(&bar("110", "90")).unwrap();
        }
        assert!(rbr.is_ready());
        rbr.reset();
        assert!(!rbr.is_ready());
    }

    #[test]
    fn test_rbr_period_and_name() {
        let rbr = RelativeBarRange::new("my_rbr", 14).unwrap();
        assert_eq!(rbr.period(), 14);
        assert_eq!(rbr.name(), "my_rbr");
    }
}
