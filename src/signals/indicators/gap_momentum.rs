//! Gap Momentum — rolling sum of open gaps normalized by average bar range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Gap Momentum — `sum(open - prev_close) / avg_range(period)` over the last `period` bars.
///
/// Captures the cumulative overnight/session gap pressure relative to typical bar size:
/// - **Positive**: persistent upward gaps (gap-up bias).
/// - **Negative**: persistent downward gaps (gap-down bias).
/// - **Near 0**: no directional gap tendency.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen,
/// or when average range is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GapMomentum;
/// use fin_primitives::signals::Signal;
/// let gm = GapMomentum::new("gap_mom_10", 10).unwrap();
/// assert_eq!(gm.period(), 10);
/// ```
pub struct GapMomentum {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    gaps: VecDeque<Decimal>,
    ranges: VecDeque<Decimal>,
    gap_sum: Decimal,
    range_sum: Decimal,
}

impl GapMomentum {
    /// Constructs a new `GapMomentum`.
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
            gaps: VecDeque::with_capacity(period),
            ranges: VecDeque::with_capacity(period),
            gap_sum: Decimal::ZERO,
            range_sum: Decimal::ZERO,
        })
    }
}

impl Signal for GapMomentum {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.gaps.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;

        if let Some(prev) = self.prev_close {
            let gap = bar.open - prev;

            self.gap_sum += gap;
            self.range_sum += range;
            self.gaps.push_back(gap);
            self.ranges.push_back(range);

            if self.gaps.len() > self.period {
                let og = self.gaps.pop_front().unwrap();
                let or_ = self.ranges.pop_front().unwrap();
                self.gap_sum -= og;
                self.range_sum -= or_;
            }
        }

        self.prev_close = Some(bar.close);

        if self.gaps.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.range_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let avg_range = self.range_sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if avg_range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let result = self.gap_sum
            .checked_div(avg_range)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(result))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.gaps.clear();
        self.ranges.clear();
        self.gap_sum = Decimal::ZERO;
        self.range_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_gm_invalid_period() {
        assert!(GapMomentum::new("gm", 0).is_err());
    }

    #[test]
    fn test_gm_unavailable_before_warm_up() {
        let mut s = GapMomentum::new("gm", 3).unwrap();
        // First bar: seeds prev_close, no gap yet
        assert_eq!(s.update_bar(&bar("100","105","95","102")).unwrap(), SignalValue::Unavailable);
        // Second bar: 1 gap, need 3
        assert_eq!(s.update_bar(&bar("103","108","98","105")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_gm_no_gap_gives_zero() {
        let mut s = GapMomentum::new("gm", 2).unwrap();
        // Bars open exactly at prior close → gaps = 0
        s.update_bar(&bar("100","110","90","100")).unwrap();
        s.update_bar(&bar("100","110","90","100")).unwrap();
        let v = s.update_bar(&bar("100","110","90","100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert_eq!(r, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gm_positive_gaps() {
        let mut s = GapMomentum::new("gm", 2).unwrap();
        // persistent gap-ups
        s.update_bar(&bar("100","110","90","100")).unwrap();
        s.update_bar(&bar("102","112","92","102")).unwrap(); // gap=+2
        let v = s.update_bar(&bar("104","114","94","104")).unwrap(); // gap=+2; sum=4, avg_range=20
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(0), "positive gaps should give positive momentum: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_gm_reset() {
        let mut s = GapMomentum::new("gm", 2).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100","110","90","100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
