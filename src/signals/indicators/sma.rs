//! Simple Moving Average (SMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Simple Moving Average over the last `period` closing prices.
///
/// Returns `SignalValue::Unavailable` until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Sma;
/// use fin_primitives::signals::Signal;
/// let sma = Sma::new("sma_20", 20).unwrap();
/// assert_eq!(sma.period(), 20);
/// ```
pub struct Sma {
    name: String,
    period: usize,
    values: VecDeque<Decimal>,
}

impl Sma {
    /// Constructs a new `Sma` with the given name and period.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, crate::error::FinError> {
        if period == 0 {
            return Err(crate::error::FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            values: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Sma {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.values.push_back(bar.close);
        if self.values.len() > self.period {
            self.values.pop_front();
        }
        if self.values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let sum: Decimal = self.values.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.values.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.values.clear();
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
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_sma_new_period_zero_fails() {
        assert!(matches!(
            Sma::new("sma0", 0),
            Err(crate::error::FinError::InvalidPeriod(0))
        ));
    }

    #[test]
    fn test_sma_not_ready_before_period() {
        let mut sma = Sma::new("sma3", 3).unwrap();
        sma.update_bar(&bar("10")).unwrap();
        sma.update_bar(&bar("20")).unwrap();
        assert!(!sma.is_ready());
        let val = sma.update_bar(&bar("10")).unwrap();
        assert!(sma.is_ready());
        assert!(matches!(val, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_sma_unavailable_before_period() {
        let mut sma = Sma::new("sma3", 3).unwrap();
        let v1 = sma.update_bar(&bar("10")).unwrap();
        assert_eq!(v1, SignalValue::Unavailable);
    }

    #[test]
    fn test_sma_value_correct_after_period() {
        let mut sma = Sma::new("sma3", 3).unwrap();
        sma.update_bar(&bar("10")).unwrap();
        sma.update_bar(&bar("20")).unwrap();
        let v = sma.update_bar(&bar("30")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_sma_rolls_window() {
        let mut sma = Sma::new("sma3", 3).unwrap();
        sma.update_bar(&bar("10")).unwrap();
        sma.update_bar(&bar("20")).unwrap();
        sma.update_bar(&bar("30")).unwrap();
        let v = sma.update_bar(&bar("40")).unwrap();
        // window is [20, 30, 40] → avg = 30
        assert_eq!(v, SignalValue::Scalar(dec!(30)));
    }

    #[test]
    fn test_sma_constant_series_equals_constant() {
        let mut sma = Sma::new("sma5", 5).unwrap();
        for _ in 0..4 {
            sma.update_bar(&bar("77")).unwrap();
        }
        let v = sma.update_bar(&bar("77")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(77)));
    }

    #[test]
    fn test_sma_empty_input_not_ready() {
        let sma = Sma::new("sma3", 3).unwrap();
        assert!(!sma.is_ready());
    }

    #[test]
    fn test_sma_window_larger_than_data_returns_unavailable() {
        let mut sma = Sma::new("sma10", 10).unwrap();
        sma.update_bar(&bar("100")).unwrap();
        let v = sma.update_bar(&bar("200")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
        assert!(!sma.is_ready());
    }

    #[test]
    fn test_sma_period_1_immediate_readiness() {
        let mut sma = Sma::new("sma1", 1).unwrap();
        let v = sma.update_bar(&bar("55")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(55)));
        assert!(sma.is_ready());
    }

    #[test]
    fn test_sma_reset_clears_state() {
        let mut sma = Sma::new("sma3", 3).unwrap();
        sma.update_bar(&bar("10")).unwrap();
        sma.update_bar(&bar("20")).unwrap();
        sma.update_bar(&bar("30")).unwrap();
        assert!(sma.is_ready());
        sma.reset();
        assert!(!sma.is_ready());
        // After reset, still needs 3 bars
        let v1 = sma.update_bar(&bar("10")).unwrap();
        assert_eq!(v1, SignalValue::Unavailable);
        sma.update_bar(&bar("20")).unwrap();
        let v3 = sma.update_bar(&bar("30")).unwrap();
        assert_eq!(v3, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_sma_update_with_bar_input_directly() {
        let mut sma = Sma::new("sma2", 2).unwrap();
        sma.update(&BarInput {
            close: dec!(10),
            high: dec!(10),
            low: dec!(10),
            open: dec!(10),
            volume: dec!(0),
        })
        .unwrap();
        let v = sma
            .update(&BarInput {
                close: dec!(20),
                high: dec!(20),
                low: dec!(20),
                open: dec!(20),
                volume: dec!(0),
            })
            .unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(15)));
    }
}
