//! Simple Moving Average (SMA) indicator.

use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use crate::signals::{Signal, SignalValue};
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
/// let sma = Sma::new("sma_20", 20);
/// assert_eq!(sma.period(), 20);
/// ```
pub struct Sma {
    name: String,
    period: usize,
    values: VecDeque<Decimal>,
}

impl Sma {
    /// Constructs a new `Sma` with the given name and period.
    pub fn new(name: impl Into<String>, period: usize) -> Self {
        Self {
            name: name.into(),
            period,
            values: VecDeque::with_capacity(period),
        }
    }
}

impl Signal for Sma {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &OhlcvBar) -> Result<SignalValue, FinError> {
        self.values.push_back(bar.close.value());
        if self.values.len() > self.period {
            self.values.pop_front();
        }
        if self.values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let sum: Decimal = self.values.iter().copied().sum();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
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
            ts_open: NanoTimestamp(0),
            ts_close: NanoTimestamp(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_sma_not_ready_before_period() {
        let mut sma = Sma::new("sma3", 3);
        sma.update(&bar("10")).unwrap();
        sma.update(&bar("20")).unwrap();
        assert!(!sma.is_ready());
        let val = sma.update(&bar("10")).unwrap();
        assert!(sma.is_ready());
        assert!(matches!(val, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_sma_unavailable_before_period() {
        let mut sma = Sma::new("sma3", 3);
        let v1 = sma.update(&bar("10")).unwrap();
        assert!(matches!(v1, SignalValue::Unavailable));
    }

    #[test]
    fn test_sma_value_correct_after_period() {
        let mut sma = Sma::new("sma3", 3);
        sma.update(&bar("10")).unwrap();
        sma.update(&bar("20")).unwrap();
        let v = sma.update(&bar("30")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert_eq!(val, dec!(20));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sma_rolls_window() {
        let mut sma = Sma::new("sma3", 3);
        sma.update(&bar("10")).unwrap();
        sma.update(&bar("20")).unwrap();
        sma.update(&bar("30")).unwrap();
        let v = sma.update(&bar("40")).unwrap();
        // window is [20, 30, 40] → avg = 30
        if let SignalValue::Scalar(val) = v {
            assert_eq!(val, dec!(30));
        } else {
            panic!("expected Scalar");
        }
    }
}
