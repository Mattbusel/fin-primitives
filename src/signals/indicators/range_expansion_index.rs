//! Range Expansion Index indicator -- current range vs its rolling average.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Expansion Index -- how much the current bar's range deviates from its
/// N-period average, expressed as a percentage.
///
/// ```text
/// range[t]   = high[t] - low[t]
/// avg[t]     = SMA(range, period)
/// rei[t]     = (range[t] - avg[t]) / avg[t] x 100
/// ```
///
/// Positive values indicate the current bar has a wider-than-average range (range expansion).
/// Negative values indicate range contraction.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or if avg is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeExpansionIndex;
/// use fin_primitives::signals::Signal;
/// let rei = RangeExpansionIndex::new("rei", 14).unwrap();
/// assert_eq!(rei.period(), 14);
/// ```
pub struct RangeExpansionIndex {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl RangeExpansionIndex {
    /// Constructs a new `RangeExpansionIndex`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RangeExpansionIndex {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.window.push_back(range);
        self.sum += range;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let avg = self.sum / Decimal::from(self.period as u32);
        if avg.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar((range - avg) / avg * Decimal::ONE_HUNDRED))
    }

    fn reset(&mut self) {
        self.window.clear();
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
    fn test_rei_period_0_error() { assert!(RangeExpansionIndex::new("r", 0).is_err()); }

    #[test]
    fn test_rei_unavailable_before_period() {
        let mut r = RangeExpansionIndex::new("r", 3).unwrap();
        assert_eq!(r.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rei_constant_range_is_zero() {
        // range always 20 -> REI = (20 - 20) / 20 * 100 = 0
        let mut r = RangeExpansionIndex::new("r", 3).unwrap();
        r.update_bar(&bar("110", "90")).unwrap();
        r.update_bar(&bar("110", "90")).unwrap();
        let v = r.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rei_expansion_positive() {
        let mut r = RangeExpansionIndex::new("r", 3).unwrap();
        // small ranges: 10 each
        r.update_bar(&bar("110", "100")).unwrap();
        r.update_bar(&bar("110", "100")).unwrap();
        // large spike: range=40 -> avg=(10+10+40)/3, rei = (40-avg)/avg*100 > 0
        let v = r.update_bar(&bar("140", "100")).unwrap();
        if let SignalValue::Scalar(rei) = v {
            assert!(rei > dec!(0), "expected positive REI for range expansion, got {rei}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rei_reset() {
        let mut r = RangeExpansionIndex::new("r", 2).unwrap();
        r.update_bar(&bar("110", "90")).unwrap();
        r.update_bar(&bar("110", "90")).unwrap();
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
