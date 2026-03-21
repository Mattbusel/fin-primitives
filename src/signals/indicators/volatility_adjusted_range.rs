//! Volatility-Adjusted Range indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volatility-Adjusted Range.
///
/// Normalises the current bar's range (high − low) by the rolling average range,
/// producing a dimensionless measure of relative range expansion or contraction.
///
/// Formula: `var = (range_t − mean_range) / mean_range × 100`
///
/// - Positive values indicate the bar's range is above average (expansion).
/// - Negative values indicate below-average range (contraction).
///
/// Returns `SignalValue::Unavailable` until `period` bars have been accumulated.
/// Returns `SignalValue::Scalar(0.0)` when the mean range is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityAdjustedRange;
/// use fin_primitives::signals::Signal;
/// let var = VolatilityAdjustedRange::new("var_14", 14).unwrap();
/// assert_eq!(var.period(), 14);
/// ```
pub struct VolatilityAdjustedRange {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
}

impl VolatilityAdjustedRange {
    /// Constructs a new `VolatilityAdjustedRange` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, ranges: VecDeque::with_capacity(period) })
    }
}

impl Signal for VolatilityAdjustedRange {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.ranges.push_back(range);
        if self.ranges.len() > self.period {
            self.ranges.pop_front();
        }
        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.ranges.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let mean = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if mean.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let adjusted = (range - mean)
            .checked_div(mean)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(adjusted))
    }

    fn is_ready(&self) -> bool {
        self.ranges.len() >= self.period
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
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l, high: h, low: l, close: h,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(VolatilityAdjustedRange::new("var", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut var = VolatilityAdjustedRange::new("var", 3).unwrap();
        assert_eq!(var.update_bar(&bar("12", "9")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_average_range_zero_return() {
        let mut var = VolatilityAdjustedRange::new("var", 3).unwrap();
        for _ in 0..3 {
            var.update_bar(&bar("10", "7")).unwrap(); // range=3
        }
        // Same range → 0%
        let v = var.update_bar(&bar("10", "7")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_wider_range_positive() {
        let mut var = VolatilityAdjustedRange::new("var", 3).unwrap();
        for _ in 0..3 {
            var.update_bar(&bar("10", "8")).unwrap(); // range=2
        }
        // Wider range (6 vs mean 2 → +200%)
        let v = var.update_bar(&bar("10", "4")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_narrower_range_negative() {
        let mut var = VolatilityAdjustedRange::new("var", 3).unwrap();
        for _ in 0..3 {
            var.update_bar(&bar("10", "4")).unwrap(); // range=6
        }
        // Narrow range (2 vs mean 6 → -66.7%)
        let v = var.update_bar(&bar("10", "8")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s < dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut var = VolatilityAdjustedRange::new("var", 2).unwrap();
        var.update_bar(&bar("10", "7")).unwrap();
        var.update_bar(&bar("10", "7")).unwrap();
        assert!(var.is_ready());
        var.reset();
        assert!(!var.is_ready());
    }
}
