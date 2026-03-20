//! Aroon indicator — measures trend strength and direction.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Aroon indicator over `period` bars.
///
/// Produces a two-value output packed into a single `Scalar` as `aroon_up - aroon_down`
/// (the Aroon Oscillator). Use [`Aroon::up`] and [`Aroon::down`] for individual values.
///
/// ```text
/// Aroon Up   = (period - bars since highest high in period) / period × 100
/// Aroon Down = (period - bars since lowest low  in period) / period × 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Aroon;
/// use fin_primitives::signals::Signal;
///
/// let mut aroon = Aroon::new("aroon14", 14).unwrap();
/// ```
pub struct Aroon {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl Aroon {
    /// Constructs a new `Aroon` indicator.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period` is zero.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            highs: VecDeque::with_capacity(period + 1),
            lows: VecDeque::with_capacity(period + 1),
        })
    }

    /// Returns the most recent Aroon Up value (0–100), or `None` before warmup.
    pub fn up(&self) -> Option<Decimal> {
        if !self.is_ready() {
            return None;
        }
        let (idx, _) = self
            .highs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.cmp(b.1))?;
        let bars_since = (self.highs.len() - 1) - idx;
        Some(
            (Decimal::from(self.period as u64)
                - Decimal::from(bars_since as u64))
                / Decimal::from(self.period as u64)
                * Decimal::ONE_HUNDRED,
        )
    }

    /// Returns the most recent Aroon Down value (0–100), or `None` before warmup.
    pub fn down(&self) -> Option<Decimal> {
        if !self.is_ready() {
            return None;
        }
        let (idx, _) = self
            .lows
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.cmp(b.1))?;
        let bars_since = (self.lows.len() - 1) - idx;
        Some(
            (Decimal::from(self.period as u64)
                - Decimal::from(bars_since as u64))
                / Decimal::from(self.period as u64)
                * Decimal::ONE_HUNDRED,
        )
    }
}

impl Signal for Aroon {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period + 1 {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        match (self.up(), self.down()) {
            (Some(u), Some(d)) => Ok(SignalValue::Scalar(u - d)),
            _ => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period + 1
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;
    use rust_decimal_macros::dec;

    fn bar(high: &str, low: &str) -> BarInput {
        // BarInput::new(close, high, low, open, volume)
        let h: Decimal = high.parse().unwrap();
        let l: Decimal = low.parse().unwrap();
        let mid = (h + l) / Decimal::TWO;
        BarInput::new(mid, h, l, mid, Decimal::ZERO)
    }

    #[test]
    fn test_aroon_period_zero_error() {
        assert!(Aroon::new("aroon", 0).is_err());
    }

    #[test]
    fn test_aroon_unavailable_before_period_plus_one() {
        let mut aroon = Aroon::new("aroon2", 2).unwrap();
        let r1 = aroon.update(&bar("110", "90")).unwrap();
        assert_eq!(r1, SignalValue::Unavailable);
        let r2 = aroon.update(&bar("112", "92")).unwrap();
        assert_eq!(r2, SignalValue::Unavailable);
    }

    #[test]
    fn test_aroon_ready_after_period_plus_one() {
        let mut aroon = Aroon::new("aroon2", 2).unwrap();
        aroon.update(&bar("110", "90")).unwrap();
        aroon.update(&bar("112", "92")).unwrap();
        let r = aroon.update(&bar("108", "88")).unwrap();
        assert!(matches!(r, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_aroon_up_returns_100_when_high_at_latest_bar() {
        let mut aroon = Aroon::new("aroon2", 2).unwrap();
        aroon.update(&bar("100", "90")).unwrap();
        aroon.update(&bar("105", "95")).unwrap();
        // Third bar is new high — up should be 100
        aroon.update(&bar("110", "98")).unwrap();
        assert_eq!(aroon.up(), Some(dec!(100)));
    }

    #[test]
    fn test_aroon_reset_clears_state() {
        let mut aroon = Aroon::new("aroon2", 2).unwrap();
        aroon.update(&bar("110", "90")).unwrap();
        aroon.update(&bar("112", "92")).unwrap();
        aroon.update(&bar("108", "88")).unwrap();
        aroon.reset();
        assert!(!aroon.is_ready());
        assert!(aroon.up().is_none());
    }

    #[test]
    fn test_aroon_period_accessor() {
        let aroon = Aroon::new("aroon14", 14).unwrap();
        assert_eq!(aroon.period(), 14);
    }

    #[test]
    fn test_aroon_name_accessor() {
        let aroon = Aroon::new("my_aroon", 5).unwrap();
        assert_eq!(aroon.name(), "my_aroon");
    }
}
