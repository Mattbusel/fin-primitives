//! Average Gap — rolling mean absolute gap between open and prior close, normalized by ATR.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Average Gap — `mean(|open - prev_close|) / mean(true_range)` over `period` bars.
///
/// Measures the typical session-open gap size relative to average bar volatility:
/// - **High value**: frequent or large opening gaps (overnight risk / gap-prone instrument).
/// - **Low value (near 0)**: opens close to prior close (continuous/liquid instrument).
///
/// Uses `(high - low)` as a simple range proxy (no gap adjustment). Returns
/// [`SignalValue::Unavailable`] until `period + 1` bars have been seen, or when
/// average range is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AverageGap;
/// use fin_primitives::signals::Signal;
/// let ag = AverageGap::new("avg_gap_10", 10).unwrap();
/// assert_eq!(ag.period(), 10);
/// ```
pub struct AverageGap {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    gaps: VecDeque<Decimal>,
    ranges: VecDeque<Decimal>,
    gap_sum: Decimal,
    range_sum: Decimal,
}

impl AverageGap {
    /// Constructs a new `AverageGap`.
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

impl Signal for AverageGap {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.gaps.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;

        if let Some(prev) = self.prev_close {
            let gap = if bar.open >= prev { bar.open - prev } else { prev - bar.open };

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

        let avg_gap = self.gap_sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let ratio = avg_gap
            .checked_div(avg_range)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio.max(Decimal::ZERO)))
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
    fn test_ag_invalid_period() {
        assert!(AverageGap::new("ag", 0).is_err());
    }

    #[test]
    fn test_ag_unavailable_before_warm_up() {
        let mut s = AverageGap::new("ag", 2).unwrap();
        assert_eq!(s.update_bar(&bar("100","110","90","100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("100","110","90","100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ag_no_gap_gives_zero() {
        let mut s = AverageGap::new("ag", 2).unwrap();
        // Each bar opens at prior close → gaps = 0
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
    fn test_ag_positive_gap() {
        let mut s = AverageGap::new("ag", 1).unwrap();
        s.update_bar(&bar("100","110","90","100")).unwrap(); // seeds prev_close=100
        // Next bar opens at 105 → gap=5, range=20 → ratio=0.25
        let v = s.update_bar(&bar("105","115","95","105")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(0), "gap should be positive: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ag_non_negative() {
        let mut s = AverageGap::new("ag", 3).unwrap();
        let bars = [
            bar("100","110","90","102"),
            bar("103","112","93","105"),
            bar("104","115","95","108"),
            bar("110","120","100","112"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = s.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "average gap ratio must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_ag_reset() {
        let mut s = AverageGap::new("ag", 2).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100","110","90","100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
