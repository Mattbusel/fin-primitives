//! Percent Rank Range indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Percent Rank Range — the percentile rank of the current bar's range (`high - low`)
/// within the last `period` bar ranges.
///
/// ```text
/// range[i]         = high[i] - low[i]
/// rank[t]          = count(range[i] < range[t],  i in window) / period × 100
/// ```
///
/// - **100**: current bar has the widest range seen in the window (high volatility).
/// - **0**: current bar has the narrowest range (low volatility / compression).
/// - **50**: median range.
///
/// Useful for detecting volatility extremes: a very high rank suggests a potential
/// volatility exhaustion, while a very low rank indicates compression before a breakout.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PercentRankRange;
/// use fin_primitives::signals::Signal;
/// let prr = PercentRankRange::new("prr_14", 14).unwrap();
/// assert_eq!(prr.period(), 14);
/// ```
pub struct PercentRankRange {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl PercentRankRange {
    /// Constructs a new `PercentRankRange`.
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
        })
    }
}

impl Signal for PercentRankRange {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ranges.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let current_range = bar.range();

        // Rank the current range against the existing window before adding it
        let rank = if self.ranges.is_empty() {
            Decimal::ZERO
        } else {
            let count_below = self.ranges.iter().filter(|&&r| r < current_range).count();
            Decimal::from(count_below as u32)
                / Decimal::from(self.ranges.len() as u32)
                * Decimal::ONE_HUNDRED
        };

        self.ranges.push_back(current_range);
        if self.ranges.len() > self.period {
            self.ranges.pop_front();
        }

        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        Ok(SignalValue::Scalar(rank))
    }

    fn reset(&mut self) {
        self.ranges.clear();
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
    fn test_prr_invalid_period() {
        assert!(PercentRankRange::new("prr", 0).is_err());
    }

    #[test]
    fn test_prr_unavailable_during_warmup() {
        let mut prr = PercentRankRange::new("prr", 3).unwrap();
        assert_eq!(prr.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(prr.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!prr.is_ready());
    }

    #[test]
    fn test_prr_uniform_ranges_fifty() {
        // All equal ranges → rank = 0 (none strictly below)
        let mut prr = PercentRankRange::new("prr", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..4 {
            last = prr.update_bar(&bar("110", "90")).unwrap(); // range=20
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_prr_widest_bar_high_rank() {
        // Three narrow bars (range=5) then one very wide bar (range=50) → high rank
        let mut prr = PercentRankRange::new("prr", 3).unwrap();
        prr.update_bar(&bar("105", "100")).unwrap(); // range=5
        prr.update_bar(&bar("105", "100")).unwrap(); // range=5
        prr.update_bar(&bar("105", "100")).unwrap(); // range=5 — period complete
        // Now push a wide bar: range=50
        if let SignalValue::Scalar(v) = prr.update_bar(&bar("150", "100")).unwrap() {
            assert!(v > dec!(50), "wide bar should rank high: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_prr_narrowest_bar_low_rank() {
        // Three wide bars (range=50), then one narrow bar (range=5) → low rank
        let mut prr = PercentRankRange::new("prr", 3).unwrap();
        prr.update_bar(&bar("150", "100")).unwrap(); // range=50
        prr.update_bar(&bar("150", "100")).unwrap(); // range=50
        prr.update_bar(&bar("150", "100")).unwrap(); // range=50 — period complete
        // Now push a narrow bar: range=5
        if let SignalValue::Scalar(v) = prr.update_bar(&bar("105", "100")).unwrap() {
            assert!(v == dec!(0), "narrow bar should rank 0 (none below): {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_prr_reset() {
        let mut prr = PercentRankRange::new("prr", 3).unwrap();
        for _ in 0..3 { prr.update_bar(&bar("110", "90")).unwrap(); }
        assert!(prr.is_ready());
        prr.reset();
        assert!(!prr.is_ready());
    }
}
