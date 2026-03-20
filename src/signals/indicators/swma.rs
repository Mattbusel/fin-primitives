//! Sine-Weighted Moving Average (SWMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Sine-Weighted Moving Average over `period` bars.
///
/// Each of the `period` bars is weighted by `sin(i * π / (period + 1))` where `i` is the
/// 1-based position (oldest = 1, newest = period).  The weights naturally peak in the
/// middle of the window, giving more emphasis to the centre of the look-back.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Swma;
/// use fin_primitives::signals::Signal;
///
/// let swma = Swma::new("swma10", 10).unwrap();
/// assert_eq!(swma.period(), 10);
/// ```
pub struct Swma {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
    weights: Vec<f64>,
    weight_sum: f64,
}

impl Swma {
    /// Constructs a new `Swma` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        use std::f64::consts::PI;
        let weights: Vec<f64> = (1..=period)
            .map(|i| (i as f64 * PI / (period as f64 + 1.0)).sin())
            .collect();
        let weight_sum: f64 = weights.iter().sum();
        Ok(Self {
            name: name.into(),
            period,
            history: VecDeque::with_capacity(period),
            weights,
            weight_sum,
        })
    }
}

impl Signal for Swma {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() > self.period {
            self.history.pop_front();
        }
        if self.history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mut weighted_sum = 0.0f64;
        for (i, &close) in self.history.iter().enumerate() {
            use rust_decimal::prelude::ToPrimitive;
            let price_f64 = close.to_f64().unwrap_or(0.0);
            weighted_sum += price_f64 * self.weights[i];
        }
        let result = weighted_sum / self.weight_sum;
        Ok(SignalValue::Scalar(
            Decimal::try_from(result).unwrap_or(Decimal::ZERO),
        ))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

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
    fn test_swma_invalid_period() {
        assert!(Swma::new("s", 0).is_err());
    }

    #[test]
    fn test_swma_unavailable_before_period() {
        let mut s = Swma::new("s3", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_swma_constant_price_equals_price() {
        // When all prices are the same, SWMA == that price.
        let mut s = Swma::new("s3", 3).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("100")).unwrap();
        let v = s.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(val) = v {
            // Allow tiny floating-point rounding; should be ~100
            let diff = (val - rust_decimal_macros::dec!(100)).abs();
            assert!(diff < rust_decimal_macros::dec!(0.0001), "Expected ~100, got {val}");
        } else {
            panic!("Expected Scalar");
        }
    }

    #[test]
    fn test_swma_is_ready_after_period_bars() {
        let mut s = Swma::new("s2", 2).unwrap();
        s.update_bar(&bar("100")).unwrap();
        assert!(!s.is_ready());
        s.update_bar(&bar("101")).unwrap();
        assert!(s.is_ready());
    }

    #[test]
    fn test_swma_reset() {
        let mut s = Swma::new("s2", 2).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
