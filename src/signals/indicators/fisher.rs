//! Fisher Transform indicator.
//!
//! Converts price into a Gaussian normal distribution, making turning points
//! easier to identify. Values above +1.5 suggest an overbought condition;
//! values below −1.5 suggest oversold.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Fisher Transform: converts the highest/lowest price range into a normal distribution.
///
/// Formula:
/// - `x = clamp(2 * (close - lowest_low) / (highest_high - lowest_low) - 1, -0.999, 0.999)`
/// - `fisher = 0.5 * ln((1 + x) / (1 - x))`
///
/// Returns [`crate::signals::SignalValue::Unavailable`] until `period` bars are seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Fisher;
/// use fin_primitives::signals::Signal;
/// let f = Fisher::new("fisher9", 9).unwrap();
/// assert_eq!(f.period(), 9);
/// assert!(!f.is_ready());
/// ```
pub struct Fisher {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl Fisher {
    /// Constructs a new `Fisher` indicator.
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
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Fisher {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.closes.len() > self.period {
            self.closes.pop_front();
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let highest: Decimal = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let lowest: Decimal = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let range = highest - lowest;
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let close = *self.closes.back().unwrap();
        // x ∈ (-0.999, 0.999)
        let raw_x = Decimal::TWO * (close - lowest) / range - Decimal::ONE;
        let clamp_limit = Decimal::new(999, 3);
        let x = raw_x.clamp(-clamp_limit, clamp_limit);

        // fisher = 0.5 * ln((1+x)/(1-x))
        use rust_decimal::prelude::ToPrimitive;
        let x_f = x.to_f64().ok_or(FinError::ArithmeticOverflow)?;
        let fisher_f = 0.5 * ((1.0 + x_f) / (1.0 - x_f)).ln();
        Decimal::try_from(fisher_f)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.highs.clear();
        self.lows.clear();
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
    fn test_fisher_invalid_period() {
        assert!(Fisher::new("f", 0).is_err());
    }

    #[test]
    fn test_fisher_unavailable_before_warmup() {
        let mut f = Fisher::new("f", 3).unwrap();
        assert!(!f.is_ready());
        f.update(&bar("105", "95", "100")).unwrap();
        assert!(!f.is_ready());
    }

    #[test]
    fn test_fisher_ready_after_period_bars() {
        let mut f = Fisher::new("f", 3).unwrap();
        f.update(&bar("105", "95", "100")).unwrap();
        f.update(&bar("108", "98", "103")).unwrap();
        let sv = f.update(&bar("110", "100", "106")).unwrap();
        assert!(f.is_ready());
        assert!(matches!(sv, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_fisher_zero_at_midpoint() {
        // When close is exactly at midpoint of range, x=0, fisher=0
        let mut f = Fisher::new("f", 1).unwrap();
        let sv = f.update(&bar("110", "90", "100")).unwrap();
        if let SignalValue::Scalar(v) = sv {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_fisher_positive_when_close_above_mid() {
        let mut f = Fisher::new("f", 1).unwrap();
        let sv = f.update(&bar("110", "90", "108")).unwrap();
        if let SignalValue::Scalar(v) = sv {
            assert!(v > dec!(0), "fisher should be positive: {}", v);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_fisher_negative_when_close_below_mid() {
        let mut f = Fisher::new("f", 1).unwrap();
        let sv = f.update(&bar("110", "90", "92")).unwrap();
        if let SignalValue::Scalar(v) = sv {
            assert!(v < dec!(0), "fisher should be negative: {}", v);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_fisher_reset_clears_state() {
        let mut f = Fisher::new("f", 2).unwrap();
        f.update(&bar("110", "90", "100")).unwrap();
        f.update(&bar("112", "92", "105")).unwrap();
        assert!(f.is_ready());
        f.reset();
        assert!(!f.is_ready());
    }

    #[test]
    fn test_fisher_flat_range_returns_unavailable() {
        let mut f = Fisher::new("f", 1).unwrap();
        // high == low == close → range is zero
        let sv = f.update(&bar("100", "100", "100")).unwrap();
        assert_eq!(sv, SignalValue::Unavailable);
    }

    #[test]
    fn test_fisher_period_accessor() {
        let f = Fisher::new("f", 9).unwrap();
        assert_eq!(f.period(), 9);
    }

    #[test]
    fn test_fisher_name_accessor() {
        let f = Fisher::new("my_fisher", 5).unwrap();
        assert_eq!(f.name(), "my_fisher");
    }
}
