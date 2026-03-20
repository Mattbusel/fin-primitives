//! Deviation from Moving Average indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Deviation from Moving Average (DeviationFromMa) — close minus SMA(close, period).
///
/// ```text
/// DeviationFromMa = close - SMA(close, period)
/// ```
///
/// Positive values indicate price is above its moving average (potential overextension).
/// Negative values indicate price is below its moving average (potential underextension).
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DeviationFromMa;
/// use fin_primitives::signals::Signal;
///
/// let d = DeviationFromMa::new("dev20", 20).unwrap();
/// assert_eq!(d.period(), 20);
/// ```
pub struct DeviationFromMa {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl DeviationFromMa {
    /// Constructs a new `DeviationFromMa`.
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
            history: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for DeviationFromMa {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() > self.period {
            self.history.pop_front();
        }
        if self.history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.history.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let sma = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(bar.close - sma))
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
    fn test_deviation_invalid_period() {
        assert!(DeviationFromMa::new("d", 0).is_err());
    }

    #[test]
    fn test_deviation_unavailable_before_period() {
        let mut d = DeviationFromMa::new("d", 3).unwrap();
        assert_eq!(d.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(d.update_bar(&bar("102")).unwrap(), SignalValue::Unavailable);
        assert!(!d.is_ready());
    }

    #[test]
    fn test_deviation_flat_is_zero() {
        let mut d = DeviationFromMa::new("d", 3).unwrap();
        d.update_bar(&bar("100")).unwrap();
        d.update_bar(&bar("100")).unwrap();
        let v = d.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_deviation_above_ma_positive() {
        // [100, 100, 110]: SMA = 310/3, close = 110, dev > 0
        let mut d = DeviationFromMa::new("d", 3).unwrap();
        d.update_bar(&bar("100")).unwrap();
        d.update_bar(&bar("100")).unwrap();
        let v = d.update_bar(&bar("110")).unwrap();
        if let SignalValue::Scalar(dev) = v {
            assert!(dev > dec!(0), "expected positive deviation, got {dev}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_deviation_below_ma_negative() {
        // [110, 110, 100]: SMA = 320/3 ~ 106.67, close = 100, dev < 0
        let mut d = DeviationFromMa::new("d", 3).unwrap();
        d.update_bar(&bar("110")).unwrap();
        d.update_bar(&bar("110")).unwrap();
        let v = d.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(dev) = v {
            assert!(dev < dec!(0), "expected negative deviation, got {dev}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_deviation_reset() {
        let mut d = DeviationFromMa::new("d", 2).unwrap();
        d.update_bar(&bar("100")).unwrap();
        d.update_bar(&bar("102")).unwrap();
        assert!(d.is_ready());
        d.reset();
        assert!(!d.is_ready());
    }
}
