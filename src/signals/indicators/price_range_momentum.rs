//! Price Range Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Range Momentum.
///
/// Measures the rate of change of the bar's intraday range (`high − low`) over
/// a rolling window. A positive value indicates that ranges are expanding
/// (increasing volatility / momentum); a negative value indicates contracting
/// ranges (compression / consolidation).
///
/// Formula: `range_roc = (range_t − range_{t−period}) / range_{t−period} × 100`
///
/// Returns `SignalValue::Unavailable` until `period + 1` ranges have been seen.
/// When the reference range is zero, returns `SignalValue::Scalar(0)`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceRangeMomentum;
/// use fin_primitives::signals::Signal;
/// let prm = PriceRangeMomentum::new("prm_10", 10).unwrap();
/// assert_eq!(prm.period(), 10);
/// ```
pub struct PriceRangeMomentum {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl PriceRangeMomentum {
    /// Constructs a new `PriceRangeMomentum` with the given name and period.
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
            ranges: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for PriceRangeMomentum {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.ranges.push_back(range);
        if self.ranges.len() > self.period + 1 {
            self.ranges.pop_front();
        }
        if self.ranges.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let current = *self.ranges.back().unwrap();
        let reference = *self.ranges.front().unwrap();

        if reference.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let roc = (current - reference)
            .checked_div(reference)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(roc))
    }

    fn is_ready(&self) -> bool {
        self.ranges.len() >= self.period + 1
    }

    fn period(&self) -> usize {
        self.period
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

    fn bar(high: &str, low: &str) -> OhlcvBar {
        let h = Price::new(high.parse().unwrap()).unwrap();
        let l = Price::new(low.parse().unwrap()).unwrap();
        let c = Price::new(high.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l,
            high: h,
            low: l,
            close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(
            PriceRangeMomentum::new("prm", 0),
            Err(FinError::InvalidPeriod(0))
        ));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut prm = PriceRangeMomentum::new("prm", 3).unwrap();
        let v = prm.update_bar(&bar("12", "9")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_ready_after_period_plus_one() {
        let mut prm = PriceRangeMomentum::new("prm", 3).unwrap();
        for _ in 0..4 {
            prm.update_bar(&bar("12", "9")).unwrap();
        }
        assert!(prm.is_ready());
    }

    #[test]
    fn test_constant_range_zero_roc() {
        let mut prm = PriceRangeMomentum::new("prm", 3).unwrap();
        for _ in 0..4 {
            prm.update_bar(&bar("12", "9")).unwrap();
        }
        let v = prm.update_bar(&bar("12", "9")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_expanding_range_positive() {
        let mut prm = PriceRangeMomentum::new("prm", 1).unwrap();
        prm.update_bar(&bar("10", "9")).unwrap(); // range = 1
        let v = prm.update_bar(&bar("12", "9")).unwrap(); // range = 3
        // ROC = (3 - 1) / 1 * 100 = 200
        assert_eq!(v, SignalValue::Scalar(dec!(200)));
    }

    #[test]
    fn test_contracting_range_negative() {
        let mut prm = PriceRangeMomentum::new("prm", 1).unwrap();
        prm.update_bar(&bar("12", "8")).unwrap(); // range = 4
        let v = prm.update_bar(&bar("11", "10")).unwrap(); // range = 1
        // ROC = (1 - 4) / 4 * 100 = -75
        assert_eq!(v, SignalValue::Scalar(dec!(-75)));
    }

    #[test]
    fn test_zero_reference_range_returns_zero() {
        let mut prm = PriceRangeMomentum::new("prm", 1).unwrap();
        prm.update_bar(&bar("10", "10")).unwrap(); // range = 0
        let v = prm.update_bar(&bar("12", "9")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset_clears_state() {
        let mut prm = PriceRangeMomentum::new("prm", 2).unwrap();
        for _ in 0..3 {
            prm.update_bar(&bar("12", "9")).unwrap();
        }
        assert!(prm.is_ready());
        prm.reset();
        assert!(!prm.is_ready());
    }
}
