//! Consolidation Score indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Consolidation Score — measures how consolidated (range-contracted) recent
/// price action is relative to a longer historical baseline.
///
/// ```text
/// avg_range_fast = SMA(range, fast)
/// avg_range_slow = SMA(range, slow)
/// score = 1 - (avg_range_fast / avg_range_slow)
/// ```
///
/// - Values near `1.0` → high consolidation (tight range relative to baseline)
/// - Values near `0.0` → no consolidation (normal range)
/// - Negative values → expansion (range wider than baseline)
///
/// Returns [`SignalValue::Unavailable`] until `slow` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ConsolidationScore;
/// use fin_primitives::signals::Signal;
///
/// let cs = ConsolidationScore::new("cs", 5, 20).unwrap();
/// assert_eq!(cs.period(), 20);
/// ```
pub struct ConsolidationScore {
    name: String,
    fast: usize,
    slow: usize,
    ranges: VecDeque<Decimal>,
}

impl ConsolidationScore {
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `fast == 0` or `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 || fast >= slow { return Err(FinError::InvalidPeriod(fast)); }
        Ok(Self { name: name.into(), fast, slow, ranges: VecDeque::with_capacity(slow) })
    }
}

impl Signal for ConsolidationScore {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.slow }
    fn is_ready(&self) -> bool { self.ranges.len() >= self.slow }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.ranges.push_back(range);
        if self.ranges.len() > self.slow { self.ranges.pop_front(); }
        if self.ranges.len() < self.slow { return Ok(SignalValue::Unavailable); }

        #[allow(clippy::cast_possible_truncation)]
        let fast_avg = self.ranges.iter().rev().take(self.fast).sum::<Decimal>()
            / Decimal::from(self.fast as u32);
        let slow_avg = self.ranges.iter().sum::<Decimal>()
            / Decimal::from(self.slow as u32);

        if slow_avg.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(Decimal::ONE - fast_avg / slow_avg))
    }

    fn reset(&mut self) { self.ranges.clear(); }
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
    fn test_cs_invalid() {
        assert!(ConsolidationScore::new("c", 0, 10).is_err());
        assert!(ConsolidationScore::new("c", 10, 5).is_err());
        assert!(ConsolidationScore::new("c", 10, 10).is_err());
    }

    #[test]
    fn test_cs_unavailable() {
        let mut cs = ConsolidationScore::new("c", 3, 5).unwrap();
        for _ in 0..4 {
            assert_eq!(cs.update_bar(&bar("110","90")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_cs_equal_ranges_gives_zero() {
        // fast and slow are identical when all ranges are same → score = 0
        let mut cs = ConsolidationScore::new("c", 3, 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = cs.update_bar(&bar("110","90")).unwrap(); }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cs_positive_in_consolidation() {
        // Wide bars for slow window, then tight bars for fast → consolidation score > 0
        let mut cs = ConsolidationScore::new("c", 3, 5).unwrap();
        for _ in 0..5 { cs.update_bar(&bar("120","80")).unwrap(); } // wide
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 { last = cs.update_bar(&bar("101","99")).unwrap(); } // tight
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "tight fast range should give positive score: {}", v);
        }
    }

    #[test]
    fn test_cs_reset() {
        let mut cs = ConsolidationScore::new("c", 3, 5).unwrap();
        for _ in 0..5 { cs.update_bar(&bar("110","90")).unwrap(); }
        cs.reset();
        assert!(!cs.is_ready());
    }
}
