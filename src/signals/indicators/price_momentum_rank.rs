//! Price Momentum Rank indicator.
//!
//! Ranks the current N-bar price momentum (close-to-close return) against its
//! own rolling history, producing a percentile in [0, 100].

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Momentum Rank: percentile rank of the N-bar return within a rolling window.
///
/// For each bar the `period`-bar close-to-close return is computed, then its
/// percentile rank is measured against the last `window` such returns (0 = lowest,
/// 100 = highest). Useful for cross-sectional momentum scoring or for detecting
/// when recent momentum is extreme relative to its own history.
///
/// Returns [`SignalValue::Unavailable`] until `period + window` bars have been
/// accumulated (enough to produce a momentum value *and* rank it).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0` or `window == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceMomentumRank;
/// use fin_primitives::signals::Signal;
///
/// let pmr = PriceMomentumRank::new("pmr", 10, 20).unwrap();
/// assert_eq!(pmr.period(), 10);
/// ```
pub struct PriceMomentumRank {
    name: String,
    period: usize,
    window: usize,
    closes: VecDeque<Decimal>,
    returns: VecDeque<Decimal>,
}

impl PriceMomentumRank {
    /// Constructs a new `PriceMomentumRank`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0` or `window == 0`.
    pub fn new(name: impl Into<String>, period: usize, window: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        if window == 0 {
            return Err(FinError::InvalidPeriod(window));
        }
        Ok(Self {
            name: name.into(),
            period,
            window,
            closes: VecDeque::with_capacity(period + 1),
            returns: VecDeque::with_capacity(window),
        })
    }

    fn percentile_rank(value: Decimal, history: &VecDeque<Decimal>) -> Decimal {
        if history.is_empty() {
            return Decimal::new(50, 0);
        }
        let count_below = history.iter().filter(|&&v| v < value).count();
        Decimal::from(count_below as u32)
            / Decimal::from(history.len() as u32)
            * Decimal::ONE_HUNDRED
    }
}

impl Signal for PriceMomentumRank {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.returns.len() >= self.window
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        self.closes.push_back(close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }

        if self.closes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Compute the period-bar return
        let old_close = *self.closes.front().unwrap_or(&close);
        if old_close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let ret = (close - old_close)
            .checked_div(old_close)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        let rank = Self::percentile_rank(ret, &self.returns);

        self.returns.push_back(ret);
        if self.returns.len() > self.window {
            self.returns.pop_front();
        }

        if self.returns.len() < self.window {
            return Ok(SignalValue::Unavailable);
        }

        Ok(SignalValue::Scalar(rank))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.returns.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_pmr_invalid_period_zero() {
        assert!(PriceMomentumRank::new("pmr", 0, 5).is_err());
    }

    #[test]
    fn test_pmr_invalid_window_zero() {
        assert!(PriceMomentumRank::new("pmr", 5, 0).is_err());
    }

    #[test]
    fn test_pmr_unavailable_during_warmup() {
        let mut pmr = PriceMomentumRank::new("pmr", 2, 3).unwrap();
        for _ in 0..4 {
            assert_eq!(pmr.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pmr_ready_after_period_plus_window() {
        let mut pmr = PriceMomentumRank::new("pmr", 2, 3).unwrap();
        for i in 0u32..5 {
            pmr.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(pmr.is_ready());
    }

    #[test]
    fn test_pmr_in_bounds() {
        let mut pmr = PriceMomentumRank::new("pmr", 2, 3).unwrap();
        for i in 0u32..10 {
            if let SignalValue::Scalar(v) = pmr.update_bar(&bar(&(100 + i).to_string())).unwrap() {
                assert!(v >= dec!(0), "rank below 0: {v}");
                assert!(v <= dec!(100), "rank above 100: {v}");
            }
        }
    }

    #[test]
    fn test_pmr_reset() {
        let mut pmr = PriceMomentumRank::new("pmr", 2, 3).unwrap();
        for i in 0u32..5 {
            pmr.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(pmr.is_ready());
        pmr.reset();
        assert!(!pmr.is_ready());
    }
}
