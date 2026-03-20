//! Linear Regression Slope — slope of the best-fit line through N closing prices.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Linear Regression Slope — slope of the OLS line through the last `period` closes.
///
/// Fits `close = a + b * t` over the last `period` bars using least-squares,
/// and returns the slope `b` (price change per bar):
/// - **Positive**: prices trending upward.
/// - **Negative**: prices trending downward.
/// - **Near zero**: prices moving sideways.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen,
/// or when the slope cannot be computed (zero variance in time).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LinearRegressionSlope;
/// use fin_primitives::signals::Signal;
/// let lrs = LinearRegressionSlope::new("lrs_14", 14).unwrap();
/// assert_eq!(lrs.period(), 14);
/// ```
pub struct LinearRegressionSlope {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl LinearRegressionSlope {
    /// Constructs a new `LinearRegressionSlope`.
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
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for LinearRegressionSlope {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        // t = 0, 1, ..., n-1
        let t_mean = (n - 1.0) / 2.0;
        let mut sum_tx = 0.0_f64;
        let mut sum_tt = 0.0_f64;

        for (i, c) in self.closes.iter().enumerate() {
            let t = i as f64;
            let x = match c.to_f64() {
                Some(v) => v,
                None => return Ok(SignalValue::Unavailable),
            };
            let dt = t - t_mean;
            sum_tx += dt * x;
            sum_tt += dt * dt;
        }

        if sum_tt == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let slope = sum_tx / sum_tt;

        Decimal::try_from(slope)
            .map(SignalValue::Scalar)
            .or(Ok(SignalValue::Unavailable))
    }

    fn reset(&mut self) {
        self.closes.clear();
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
    fn test_lrs_invalid_period() {
        assert!(LinearRegressionSlope::new("lrs", 0).is_err());
        assert!(LinearRegressionSlope::new("lrs", 1).is_err());
    }

    #[test]
    fn test_lrs_unavailable_before_period() {
        let mut s = LinearRegressionSlope::new("lrs", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_lrs_perfect_uptrend_gives_positive_slope() {
        // Prices 100, 101, 102 → slope = 1.0
        let mut s = LinearRegressionSlope::new("lrs", 3).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("102")).unwrap() {
            assert!((v - dec!(1)).abs() < dec!(0.001), "perfect uptrend slope should be 1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_lrs_perfect_downtrend_gives_negative_slope() {
        let mut s = LinearRegressionSlope::new("lrs", 3).unwrap();
        s.update_bar(&bar("102")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100")).unwrap() {
            assert!(v < dec!(0), "downtrend should give negative slope: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_lrs_flat_gives_near_zero() {
        let mut s = LinearRegressionSlope::new("lrs", 4).unwrap();
        for _ in 0..4 { s.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = s.update_bar(&bar("100")).unwrap() {
            assert!(v.abs() < dec!(0.001), "flat prices should give ~zero slope: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_lrs_reset() {
        let mut s = LinearRegressionSlope::new("lrs", 3).unwrap();
        for p in &["100","101","102"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
