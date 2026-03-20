//! Linear Regression Forecast indicator — projects the OLS line one bar ahead.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Linear Regression Forecast — fits an OLS line through the last `period` closes
/// and extrapolates one bar into the future.
///
/// Where [`crate::signals::indicators::LinRegSlope`] reports the slope, this indicator
/// reports the *predicted close for the next bar*. This is the value of the regression
/// line at `x = period` (one step beyond the most-recent bar at `x = period - 1`).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LinRegForecast;
/// use fin_primitives::signals::Signal;
/// let lrf = LinRegForecast::new("lrf20", 20).unwrap();
/// assert_eq!(lrf.period(), 20);
/// ```
pub struct LinRegForecast {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl LinRegForecast {
    /// Constructs a new `LinRegForecast`.
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

impl Signal for LinRegForecast {
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

        let sum_i: f64 = (0..self.period).map(|i| i as f64).sum();
        let sum_i2: f64 = (0..self.period).map(|i| (i * i) as f64).sum();
        let sum_y: f64 = y_vals.iter().sum();
        let sum_iy: f64 = y_vals
            .iter()
            .enumerate()
            .map(|(i, &y)| i as f64 * y)
            .sum();

        let denom = n * sum_i2 - sum_i * sum_i;
        let forecast = if denom == 0.0 {
            // Flat series — forecast equals the last close.
            y_vals[y_vals.len() - 1]
        } else {
            let slope = (n * sum_iy - sum_i * sum_y) / denom;
            let intercept = (sum_y - slope * sum_i) / n;
            // Project to x = period (one bar beyond the last observed x = period - 1).
            intercept + slope * n
        };

        let result = Decimal::try_from(forecast).unwrap_or(Decimal::ZERO);
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
    fn test_lrf_invalid_period() {
        assert!(LinRegForecast::new("lrf", 0).is_err());
        assert!(LinRegForecast::new("lrf", 1).is_err());
    }

    #[test]
    fn test_lrf_unavailable_before_period() {
        let mut lrf = LinRegForecast::new("lrf", 3).unwrap();
        assert_eq!(lrf.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(lrf.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!lrf.is_ready());
    }

    #[test]
    fn test_lrf_perfect_uptrend_forecasts_next() {
        // y = [100, 101, 102] → slope = 1, forecast at x=3 → 103
        let mut lrf = LinRegForecast::new("lrf", 3).unwrap();
        lrf.update_bar(&bar("100")).unwrap();
        lrf.update_bar(&bar("101")).unwrap();
        let v = lrf.update_bar(&bar("102")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!((s - dec!(103)).abs() < dec!(0.001), "expected ~103, got {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_lrf_flat_series_forecasts_same() {
        let mut lrf = LinRegForecast::new("lrf", 3).unwrap();
        lrf.update_bar(&bar("50")).unwrap();
        lrf.update_bar(&bar("50")).unwrap();
        let v = lrf.update_bar(&bar("50")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!((s - dec!(50)).abs() < dec!(0.001), "expected ~50, got {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_lrf_downtrend_forecasts_lower() {
        // y = [103, 102, 101] → slope = -1, forecast at x=3 → 100
        let mut lrf = LinRegForecast::new("lrf", 3).unwrap();
        lrf.update_bar(&bar("103")).unwrap();
        lrf.update_bar(&bar("102")).unwrap();
        let v = lrf.update_bar(&bar("101")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!((s - dec!(100)).abs() < dec!(0.001), "expected ~100, got {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_lrf_reset() {
        let mut lrf = LinRegForecast::new("lrf", 2).unwrap();
        lrf.update_bar(&bar("100")).unwrap();
        lrf.update_bar(&bar("101")).unwrap();
        assert!(lrf.is_ready());
        lrf.reset();
        assert!(!lrf.is_ready());
    }

    #[test]
    fn test_lrf_period_and_name() {
        let lrf = LinRegForecast::new("my_lrf", 20).unwrap();
        assert_eq!(lrf.period(), 20);
        assert_eq!(lrf.name(), "my_lrf");
    }
}
