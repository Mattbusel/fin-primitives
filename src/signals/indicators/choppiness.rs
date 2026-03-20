//! Choppiness Index indicator.
//!
//! Measures whether the market is trending (low values) or choppy/ranging (high values).
//! Values range from 0 to 100:
//! - Near 100 (> 61.8): choppy / sideways market
//! - Near 0  (< 38.2): strongly trending market

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Choppiness Index: `100 * log10(Σ ATR(1)) / log10(n) / (highest_high - lowest_low)`.
///
/// A value above 61.8 signals a choppy/range-bound market; below 38.2 signals a strong trend.
/// Returns [`crate::signals::SignalValue::Unavailable`] until `period` bars are available.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ChoppinessIndex;
/// use fin_primitives::signals::Signal;
/// let c = ChoppinessIndex::new("chop14", 14).unwrap();
/// assert_eq!(c.period(), 14);
/// assert!(!c.is_ready());
/// ```
pub struct ChoppinessIndex {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
    /// Keeps trailing `(high, low, tr)` for the period window.
    trs: VecDeque<Decimal>,
}

impl ChoppinessIndex {
    /// Constructs a new `ChoppinessIndex` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
            prev_close: None,
            trs: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for ChoppinessIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // True range = max(high-low, |high-prev_close|, |low-prev_close|)
        let tr = if let Some(pc) = self.prev_close {
            let hl = bar.high - bar.low;
            let hpc = (bar.high - pc).abs();
            let lpc = (bar.low - pc).abs();
            hl.max(hpc).max(lpc)
        } else {
            bar.high - bar.low
        };
        self.prev_close = Some(bar.close);
        self.trs.push_back(tr);
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.trs.len() > self.period {
            self.trs.pop_front();
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.trs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let highest = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let lowest = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let range = highest - lowest;
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let atr_sum: Decimal = self.trs.iter().copied().sum();

        use rust_decimal::prelude::ToPrimitive;
        let atr_sum_f = atr_sum.to_f64().ok_or(FinError::ArithmeticOverflow)?;
        let range_f = range.to_f64().ok_or(FinError::ArithmeticOverflow)?;
        let n_f = self.period as f64;
        let chop = 100.0 * atr_sum_f.log10() / n_f.log10() / range_f;
        Decimal::try_from(chop)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn is_ready(&self) -> bool {
        self.trs.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.trs.clear();
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;
    use rust_decimal_macros::dec;

    fn bar(high: &str, low: &str, close: &str) -> BarInput {
        BarInput::new(
            close.parse().unwrap(),
            high.parse().unwrap(),
            low.parse().unwrap(),
            close.parse().unwrap(),
            dec!(1000),
        )
    }

    #[test]
    fn test_choppiness_invalid_period() {
        assert!(ChoppinessIndex::new("c", 0).is_err());
        assert!(ChoppinessIndex::new("c", 1).is_err());
    }

    #[test]
    fn test_choppiness_unavailable_before_warmup() {
        let mut c = ChoppinessIndex::new("c", 5).unwrap();
        assert!(!c.is_ready());
        c.update(&bar("105", "95", "100")).unwrap();
        assert!(!c.is_ready());
    }

    #[test]
    fn test_choppiness_ready_after_period_bars() {
        let mut c = ChoppinessIndex::new("c", 2).unwrap();
        c.update(&bar("105", "95", "100")).unwrap();
        let sv = c.update(&bar("108", "98", "103")).unwrap();
        assert!(c.is_ready());
        assert!(matches!(sv, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_choppiness_between_0_and_100() {
        let mut c = ChoppinessIndex::new("c", 3).unwrap();
        let prices = [("110", "90", "100"), ("112", "95", "108"), ("115", "100", "110")];
        let mut last = SignalValue::Unavailable;
        for (h, l, cl) in &prices {
            last = c.update(&bar(h, l, cl)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0) && v < dec!(200), "chop out of range: {}", v);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_choppiness_reset_clears_state() {
        let mut c = ChoppinessIndex::new("c", 2).unwrap();
        c.update(&bar("105", "95", "100")).unwrap();
        c.update(&bar("108", "98", "103")).unwrap();
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
        assert_eq!(c.update(&bar("105", "95", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_choppiness_period_and_name() {
        let c = ChoppinessIndex::new("my_chop", 14).unwrap();
        assert_eq!(c.period(), 14);
        assert_eq!(c.name(), "my_chop");
    }

    #[test]
    fn test_choppiness_flat_range_returns_unavailable() {
        let mut c = ChoppinessIndex::new("c", 2).unwrap();
        c.update(&bar("100", "100", "100")).unwrap();
        let sv = c.update(&bar("100", "100", "100")).unwrap();
        assert_eq!(sv, SignalValue::Unavailable);
    }
}
