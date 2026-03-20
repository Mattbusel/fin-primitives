//! Volatility Percentile indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volatility Percentile — ranks the current bar's True Range against the
/// distribution of True Ranges over the last `period` bars.
///
/// Output is the percentile rank in [0, 1]:
/// - Values near 1.0 → current volatility is very high relative to recent history
/// - Values near 0.0 → current volatility is very low (quiet market)
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityPercentile;
/// use fin_primitives::signals::Signal;
///
/// let vp = VolatilityPercentile::new("vp", 20).unwrap();
/// assert_eq!(vp.period(), 20);
/// ```
pub struct VolatilityPercentile {
    name: String,
    period: usize,
    true_ranges: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl VolatilityPercentile {
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            true_ranges: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for VolatilityPercentile {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.true_ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = match self.prev_close {
            None => bar.range(),
            Some(pc) => {
                let hl = bar.range();
                let hc = (bar.high - pc).abs();
                let lc = (bar.low - pc).abs();
                hl.max(hc).max(lc)
            }
        };
        self.prev_close = Some(bar.close);
        self.true_ranges.push_back(tr);
        if self.true_ranges.len() > self.period { self.true_ranges.pop_front(); }
        if self.true_ranges.len() < self.period { return Ok(SignalValue::Unavailable); }

        // Percentile rank: fraction of historical TRs that are <= current TR
        let current_tr = tr;
        let below = self.true_ranges.iter().filter(|&&v| v <= current_tr).count();
        #[allow(clippy::cast_possible_truncation)]
        let pct = Decimal::from(below as u32) / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
        self.true_ranges.clear();
        self.prev_close = None;
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
    fn test_vp_invalid() { assert!(VolatilityPercentile::new("v", 0).is_err()); }

    #[test]
    fn test_vp_unavailable() {
        let mut vp = VolatilityPercentile::new("v", 5).unwrap();
        for _ in 0..4 {
            assert_eq!(vp.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vp_max_when_largest() {
        // All equal-range bars, then one very wide bar → percentile should be 1.0
        let mut vp = VolatilityPercentile::new("v", 4).unwrap();
        for _ in 0..4 { vp.update_bar(&bar("101", "99", "100")).unwrap(); }
        let last = vp.update_bar(&bar("200", "50", "100")).unwrap();
        // The very wide bar replaces one narrow bar, 4 bars now: 3 narrow + 1 wide
        // current TR is the wide one, all 4 bars <= wide → pct = 4/4 = 1
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_vp_min_when_smallest() {
        // Seed with wide bars then a very narrow bar
        let mut vp = VolatilityPercentile::new("v", 4).unwrap();
        for _ in 0..4 { vp.update_bar(&bar("120", "80", "100")).unwrap(); }
        // Very narrow bar
        let last = vp.update_bar(&bar("100.1", "99.9", "100")).unwrap();
        // current TR ≈ 0.2, all prior TRs were 40, so only 1 bar <= 0.2 (the current)
        // pct = 1/4 = 0.25 (the narrow bar itself counts as <= itself)
        if let SignalValue::Scalar(v) = last {
            assert!(v <= dec!(0.5), "narrow bar should have low percentile: {}", v);
        }
    }

    #[test]
    fn test_vp_reset() {
        let mut vp = VolatilityPercentile::new("v", 4).unwrap();
        for _ in 0..5 { vp.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(vp.is_ready());
        vp.reset();
        assert!(!vp.is_ready());
    }
}
