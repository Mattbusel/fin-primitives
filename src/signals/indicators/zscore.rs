//! Z-Score indicator — rolling standardisation of close price.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Z-Score indicator.
///
/// ```text
/// ZScore = (close − SMA(period)) / StdDev(period)
/// ```
///
/// Measures how many sample standard deviations the current close is from its
/// rolling mean. Values near 0 indicate the price is at its average; extreme
/// positive/negative values indicate statistical outliers.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or
/// when the standard deviation is zero (constant prices).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Zscore;
/// use fin_primitives::signals::Signal;
/// let z = Zscore::new("z14", 14).unwrap();
/// assert_eq!(z.period(), 14);
/// assert!(!z.is_ready());
/// ```
pub struct Zscore {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl Zscore {
    /// Constructs a new `Zscore` indicator.
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
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Zscore {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        #[allow(clippy::cast_possible_truncation)]
        let n = Decimal::from(self.period as u32);
        let sum: Decimal = self.window.iter().copied().sum();
        let mean = sum / n;

        let variance: Decimal = self.window
            .iter()
            .map(|&x| {
                let diff = x - mean;
                diff * diff
            })
            .sum::<Decimal>()
            / n;

        use rust_decimal::prelude::ToPrimitive;
        let variance_f = variance.to_f64().unwrap_or(0.0);
        if variance_f <= 0.0 {
            return Ok(SignalValue::Unavailable);
        }
        let std_dev = Decimal::try_from(variance_f.sqrt()).unwrap_or(Decimal::ONE);
        if std_dev.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let z = (bar.close - mean)
            .checked_div(std_dev)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(z))
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.window.clear();
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
    fn test_zscore_zero_period_fails() {
        assert!(Zscore::new("z", 0).is_err());
    }

    #[test]
    fn test_zscore_unavailable_before_warmup() {
        let mut z = Zscore::new("z3", 3).unwrap();
        assert_eq!(z.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(z.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!z.is_ready());
    }

    #[test]
    fn test_zscore_constant_prices_unavailable() {
        // stddev = 0 → Unavailable
        let mut z = Zscore::new("z3", 3).unwrap();
        for _ in 0..5 {
            z.update_bar(&bar("100")).unwrap();
        }
        assert!(z.is_ready()); // window is full
        let v = z.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_zscore_high_value_is_positive() {
        let mut z = Zscore::new("z3", 3).unwrap();
        // Warmup with low prices
        z.update_bar(&bar("100")).unwrap();
        z.update_bar(&bar("100")).unwrap();
        // Push a high value — z-score should be clearly positive
        let v = z.update_bar(&bar("110")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val > dec!(0), "high close should give positive z-score: {val}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_zscore_low_value_is_negative() {
        let mut z = Zscore::new("z3", 3).unwrap();
        z.update_bar(&bar("110")).unwrap();
        z.update_bar(&bar("110")).unwrap();
        let v = z.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert!(val < dec!(0), "low close should give negative z-score: {val}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_zscore_reset_clears_state() {
        let mut z = Zscore::new("z3", 3).unwrap();
        for p in &["100", "110", "120"] {
            z.update_bar(&bar(p)).unwrap();
        }
        assert!(z.is_ready());
        z.reset();
        assert!(!z.is_ready());
        assert_eq!(z.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
