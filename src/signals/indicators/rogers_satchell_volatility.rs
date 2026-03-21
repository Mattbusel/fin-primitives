//! Rogers-Satchell Volatility estimator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rogers-Satchell historical volatility estimator (Rogers & Satchell 1991).
///
/// Drift-unbiased estimator that uses all four OHLC prices. Unlike close-to-close or
/// Parkinson estimators, it remains accurate in the presence of non-zero drift.
///
/// Formula per bar: `rs = ln(H/C)·ln(H/O) + ln(L/C)·ln(L/O)`
///
/// Aggregate: `σ = sqrt( (1/n) · Σ rs_i )`
///
/// Returns `SignalValue::Unavailable` until `period` valid bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RogersSatchellVolatility;
/// use fin_primitives::signals::Signal;
/// let rs = RogersSatchellVolatility::new("rs_20", 20).unwrap();
/// assert_eq!(rs.period(), 20);
/// ```
pub struct RogersSatchellVolatility {
    name: String,
    period: usize,
    rs_values: VecDeque<f64>,
}

impl RogersSatchellVolatility {
    /// Constructs a new `RogersSatchellVolatility` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, rs_values: VecDeque::with_capacity(period) })
    }
}

impl Signal for RogersSatchellVolatility {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;
        let h = bar.high.to_f64().unwrap_or(0.0);
        let l = bar.low.to_f64().unwrap_or(0.0);
        let c = bar.close.to_f64().unwrap_or(0.0);
        let o = bar.open.to_f64().unwrap_or(0.0);
        if h <= 0.0 || l <= 0.0 || c <= 0.0 || o <= 0.0 {
            return Ok(SignalValue::Unavailable);
        }
        let rs = (h / c).ln() * (h / o).ln() + (l / c).ln() * (l / o).ln();
        self.rs_values.push_back(rs);
        if self.rs_values.len() > self.period {
            self.rs_values.pop_front();
        }
        if self.rs_values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let mean = self.rs_values.iter().sum::<f64>() / self.period as f64;
        let sigma = mean.max(0.0).sqrt();
        Decimal::try_from(sigma)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn is_ready(&self) -> bool {
        self.rs_values.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.rs_values.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(
            RogersSatchellVolatility::new("rs", 0),
            Err(FinError::InvalidPeriod(0))
        ));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut rs = RogersSatchellVolatility::new("rs", 3).unwrap();
        let v = rs.update_bar(&bar("10", "12", "9", "11")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_ready_after_period() {
        let mut rs = RogersSatchellVolatility::new("rs", 2).unwrap();
        rs.update_bar(&bar("10", "12", "9", "11")).unwrap();
        let v = rs.update_bar(&bar("11", "13", "10", "12")).unwrap();
        assert!(rs.is_ready());
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_sigma_non_negative() {
        let mut rs = RogersSatchellVolatility::new("rs", 5).unwrap();
        for _ in 0..5 {
            rs.update_bar(&bar("10", "12", "9", "11")).unwrap();
        }
        let v = rs.update_bar(&bar("10", "12", "9", "11")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s >= dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_doji_bar_gives_zero() {
        // When open == close == high == low, RS component = 0.
        let mut rs = RogersSatchellVolatility::new("rs", 3).unwrap();
        for _ in 0..3 {
            rs.update_bar(&bar("10", "10", "10", "10")).unwrap();
        }
        let v = rs.update_bar(&bar("10", "10", "10", "10")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert_eq!(s, dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset_clears_state() {
        let mut rs = RogersSatchellVolatility::new("rs", 2).unwrap();
        rs.update_bar(&bar("10", "12", "9", "11")).unwrap();
        rs.update_bar(&bar("11", "13", "10", "12")).unwrap();
        assert!(rs.is_ready());
        rs.reset();
        assert!(!rs.is_ready());
    }

    #[test]
    fn test_wider_range_larger_vol() {
        let mut narrow = RogersSatchellVolatility::new("rs", 3).unwrap();
        let mut wide = RogersSatchellVolatility::new("rs", 3).unwrap();
        for _ in 0..3 {
            narrow.update_bar(&bar("100", "102", "98", "101")).unwrap();
            wide.update_bar(&bar("100", "115", "85", "101")).unwrap();
        }
        let nv = match narrow.update_bar(&bar("100", "102", "98", "101")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        let wv = match wide.update_bar(&bar("100", "115", "85", "101")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        assert!(wv > nv);
    }
}
