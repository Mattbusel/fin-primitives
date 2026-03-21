//! Volume-Price Rank indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume-Price Rank — the percentile rank of `volume × |close - open|` (the
/// "participation-weighted body") within the last `period` bars.
///
/// ```text
/// score[i]  = volume[i] × |close[i] - open[i]|
/// rank[t]   = count(score[i] < score[t], i in window) / period × 100
/// ```
///
/// - **100**: current bar has the highest volume × price action seen in the window.
/// - **0**: current bar has the lowest participation-adjusted move.
/// - **High rank + large body**: high-conviction directional move worth noting.
/// - **High rank + tiny body**: volume spike without price follow-through (climax).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumePriceRank;
/// use fin_primitives::signals::Signal;
/// let vpr = VolumePriceRank::new("vpr_20", 20).unwrap();
/// assert_eq!(vpr.period(), 20);
/// ```
pub struct VolumePriceRank {
    name: String,
    period: usize,
    scores: VecDeque<Decimal>,
}

impl VolumePriceRank {
    /// Constructs a new `VolumePriceRank`.
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
            scores: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolumePriceRank {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.scores.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = (bar.close - bar.open).abs();
        let score = bar.volume * body;

        // Rank against existing window before adding
        let rank = if self.scores.is_empty() {
            Decimal::ZERO
        } else {
            let count_below = self.scores.iter().filter(|&&s| s < score).count();
            Decimal::from(count_below as u32)
                / Decimal::from(self.scores.len() as u32)
                * Decimal::ONE_HUNDRED
        };

        self.scores.push_back(score);
        if self.scores.len() > self.period {
            self.scores.pop_front();
        }

        if self.scores.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        Ok(SignalValue::Scalar(rank))
    }

    fn reset(&mut self) {
        self.scores.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str, vol: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let hi = op.value().max(cp.value());
        let lo = op.value().min(cp.value());
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op,
            high: Price::new(hi).unwrap(),
            low: Price::new(lo).unwrap(),
            close: cp,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vpr_invalid_period() {
        assert!(VolumePriceRank::new("vpr", 0).is_err());
    }

    #[test]
    fn test_vpr_unavailable_during_warmup() {
        let mut vpr = VolumePriceRank::new("vpr", 3).unwrap();
        assert_eq!(vpr.update_bar(&bar("100", "102", "1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(vpr.update_bar(&bar("100", "102", "1000")).unwrap(), SignalValue::Unavailable);
        assert!(!vpr.is_ready());
    }

    #[test]
    fn test_vpr_zero_body_zero_score() {
        // Doji bars with some volume → score = 0 → always rank 0 vs prior
        let mut vpr = VolumePriceRank::new("vpr", 3).unwrap();
        for _ in 0..4 {
            vpr.update_bar(&bar("100", "100", "1000")).unwrap();
        }
        if let SignalValue::Scalar(v) = vpr.update_bar(&bar("100", "100", "1000")).unwrap() {
            assert_eq!(v, dec!(0), "all doji → rank 0");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vpr_high_score_high_rank() {
        // Low-score bars first, then a high-volume high-body bar → high rank
        let mut vpr = VolumePriceRank::new("vpr", 3).unwrap();
        vpr.update_bar(&bar("100", "100.1", "10")).unwrap();
        vpr.update_bar(&bar("100", "100.1", "10")).unwrap();
        vpr.update_bar(&bar("100", "100.1", "10")).unwrap();
        // Now a bar with large volume × body
        if let SignalValue::Scalar(v) = vpr.update_bar(&bar("100", "120", "10000")).unwrap() {
            assert!(v > dec!(50), "high score bar → high rank: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vpr_reset() {
        let mut vpr = VolumePriceRank::new("vpr", 3).unwrap();
        for _ in 0..3 { vpr.update_bar(&bar("100", "101", "1000")).unwrap(); }
        assert!(vpr.is_ready());
        vpr.reset();
        assert!(!vpr.is_ready());
    }
}
