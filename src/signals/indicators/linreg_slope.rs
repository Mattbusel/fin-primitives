//! Linear Regression Slope indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Linear Regression Slope — OLS slope of the best-fit line through the last `period` closes.
///
/// Positive values indicate an uptrend; negative values a downtrend.
/// The slope is expressed in price units per bar.
///
/// ```text
/// slope = (N·Σ(i·y) − Σ(i)·Σ(y)) / (N·Σ(i²) − (Σ(i))²)
/// ```
/// where `i = 0..N-1` and `y` is the close price.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LinRegSlope;
/// use fin_primitives::signals::Signal;
///
/// let lr = LinRegSlope::new("lr20", 20).unwrap();
/// assert_eq!(lr.period(), 20);
/// ```
pub struct LinRegSlope {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl LinRegSlope {
    /// Constructs a new `LinRegSlope`.
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
            history: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for LinRegSlope {
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

        use rust_decimal::prelude::ToPrimitive;
        let n = self.period as f64;
        let y_vals: Vec<f64> = self
            .history
            .iter()
            .map(|d| d.to_f64().unwrap_or(0.0))
            .collect();

        // Precompute sums
        let sum_i: f64 = (0..self.period).map(|i| i as f64).sum();
        let sum_i2: f64 = (0..self.period).map(|i| (i * i) as f64).sum();
        let sum_y: f64 = y_vals.iter().sum();
        let sum_iy: f64 = y_vals
            .iter()
            .enumerate()
            .map(|(i, &y)| i as f64 * y)
            .sum();

        let denom = n * sum_i2 - sum_i * sum_i;
        if denom == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let slope = (n * sum_iy - sum_i * sum_y) / denom;
        let result = Decimal::try_from(slope).unwrap_or(Decimal::ZERO);
        Ok(SignalValue::Scalar(result))
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
    fn test_linreg_invalid_period() {
        assert!(LinRegSlope::new("lr", 0).is_err());
        assert!(LinRegSlope::new("lr", 1).is_err());
    }

    #[test]
    fn test_linreg_unavailable_before_period() {
        let mut lr = LinRegSlope::new("lr", 3).unwrap();
        assert_eq!(lr.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(lr.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!lr.is_ready());
    }

    #[test]
    fn test_linreg_perfect_uptrend() {
        // y = [100, 101, 102] → slope = +1
        let mut lr = LinRegSlope::new("lr", 3).unwrap();
        lr.update_bar(&bar("100")).unwrap();
        lr.update_bar(&bar("101")).unwrap();
        let v = lr.update_bar(&bar("102")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!((s - dec!(1)).abs() < dec!(0.0001));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_linreg_perfect_downtrend() {
        // y = [102, 101, 100] → slope = -1
        let mut lr = LinRegSlope::new("lr", 3).unwrap();
        lr.update_bar(&bar("102")).unwrap();
        lr.update_bar(&bar("101")).unwrap();
        let v = lr.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!((s - dec!(-1)).abs() < dec!(0.0001));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_linreg_flat() {
        let mut lr = LinRegSlope::new("lr", 3).unwrap();
        lr.update_bar(&bar("100")).unwrap();
        lr.update_bar(&bar("100")).unwrap();
        let v = lr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(Decimal::ZERO));
    }

    #[test]
    fn test_linreg_reset() {
        let mut lr = LinRegSlope::new("lr", 2).unwrap();
        lr.update_bar(&bar("100")).unwrap();
        lr.update_bar(&bar("101")).unwrap();
        assert!(lr.is_ready());
        lr.reset();
        assert!(!lr.is_ready());
    }
}
