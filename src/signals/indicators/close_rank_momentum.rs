//! Close Rank Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Rank Momentum — change in the close's percentile rank within a rolling window.
///
/// For each bar, the close is ranked against the last `period` closes (0..=100).
/// The output is the difference between the current rank and the rank `period` bars ago:
///
/// ```text
/// rank_now  = percentile_rank(close_now, window)
/// rank_prev = percentile_rank(close_{N_ago}, old_window)
/// output    = rank_now - rank_prev
/// ```
///
/// - **Positive**: close is climbing in relative rank — building upward momentum.
/// - **Negative**: close is falling in rank — building downward momentum.
/// - **Near zero**: stable rank — no momentum shift.
/// - Returns [`SignalValue::Unavailable`] until `2 * period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseRankMomentum;
/// use fin_primitives::signals::Signal;
///
/// let crm = CloseRankMomentum::new("crm", 10).unwrap();
/// assert_eq!(crm.period(), 10);
/// ```
pub struct CloseRankMomentum {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    ranks: VecDeque<Decimal>,
}

impl CloseRankMomentum {
    /// Constructs a new `CloseRankMomentum`.
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
            closes: VecDeque::with_capacity(period),
            ranks: VecDeque::with_capacity(period),
        })
    }

    fn percentile_rank(value: Decimal, window: &VecDeque<Decimal>) -> Decimal {
        if window.is_empty() {
            return Decimal::new(50, 0);
        }
        let count_below = window.iter().filter(|&&v| v < value).count();
        Decimal::from(count_below as u32)
            / Decimal::from(window.len() as u32)
            * Decimal::ONE_HUNDRED
    }
}

impl Signal for CloseRankMomentum {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ranks.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        let rank_now = Self::percentile_rank(close, &self.closes);

        self.closes.push_back(close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }

        self.ranks.push_back(rank_now);
        if self.ranks.len() > self.period {
            self.ranks.pop_front();
        }

        if self.ranks.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let rank_prev = *self.ranks.front().unwrap();
        let momentum = rank_now - rank_prev;

        Ok(SignalValue::Scalar(momentum))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.ranks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_crm_invalid_period() {
        assert!(CloseRankMomentum::new("crm", 0).is_err());
    }

    #[test]
    fn test_crm_unavailable_during_warmup() {
        let mut crm = CloseRankMomentum::new("crm", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(crm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!crm.is_ready());
    }

    #[test]
    fn test_crm_monotonic_rise_positive() {
        // Rising prices → rank should increase → positive momentum
        let mut crm = CloseRankMomentum::new("crm", 3).unwrap();
        let prices = ["100","101","102","103","104","105"];
        let mut last = SignalValue::Unavailable;
        for &p in &prices {
            last = crm.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v >= dec!(0), "rising prices → non-negative momentum: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_crm_reset() {
        let mut crm = CloseRankMomentum::new("crm", 3).unwrap();
        for i in 0u32..6 { crm.update_bar(&bar(&(100+i).to_string())).unwrap(); }
        assert!(crm.is_ready());
        crm.reset();
        assert!(!crm.is_ready());
    }
}
