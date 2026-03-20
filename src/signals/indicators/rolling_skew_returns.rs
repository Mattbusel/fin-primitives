//! Rolling Skewness of Returns indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Rolling Skewness of Returns -- measures the asymmetry of the close-return distribution
/// over the last `period` bars.
///
/// Positive skew: right tail (large gains) is longer than left (mean > median).
/// Negative skew: left tail (large losses) dominates (mean < median).
///
/// Uses the sample skewness formula:
/// ```text
/// return[t] = close[t] - close[t-1]
/// skew      = (n / ((n-1)(n-2))) * sum((r - mean)^3 / std^3)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (needs `period` returns) or if standard deviation is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingSkewReturns;
/// use fin_primitives::signals::Signal;
/// let rs = RollingSkewReturns::new("rs", 20).unwrap();
/// assert_eq!(rs.period(), 20);
/// ```
pub struct RollingSkewReturns {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
}

impl RollingSkewReturns {
    /// Constructs a new `RollingSkewReturns`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 3`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 3 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
        })
    }

    fn compute_skew(returns: &VecDeque<Decimal>) -> Option<Decimal> {
        let n = returns.len();
        if n < 3 { return None; }
        let nf = n as f64;
        let xs: Vec<f64> = returns.iter().filter_map(|r| r.to_f64()).collect();
        if xs.len() != n { return None; }
        let mean = xs.iter().sum::<f64>() / nf;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (nf - 1.0);
        let std = var.sqrt();
        if std == 0.0 { return None; }
        let m3 = xs.iter().map(|x| ((x - mean) / std).powi(3)).sum::<f64>();
        let skew = (nf / ((nf - 1.0) * (nf - 2.0))) * m3;
        Decimal::try_from(skew).ok()
    }
}

impl Signal for RollingSkewReturns {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let ret = bar.close - pc;
            self.window.push_back(ret);
            if self.window.len() > self.period {
                self.window.pop_front();
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        match Self::compute_skew(&self.window) {
            Some(s) => Ok(SignalValue::Scalar(s)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) {
        self.prev_close = None;
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
    fn test_rsr_period_too_small() { assert!(RollingSkewReturns::new("rs", 2).is_err()); }

    #[test]
    fn test_rsr_unavailable_before_period() {
        let mut rs = RollingSkewReturns::new("rs", 5).unwrap();
        assert_eq!(rs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(rs.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rsr_symmetric_returns_near_zero_skew() {
        // Symmetric returns: +1, -1, +1, -1 should give skew near 0
        let mut rs = RollingSkewReturns::new("rs", 4).unwrap();
        rs.update_bar(&bar("100")).unwrap();
        rs.update_bar(&bar("101")).unwrap(); // +1
        rs.update_bar(&bar("100")).unwrap(); // -1
        rs.update_bar(&bar("101")).unwrap(); // +1
        let r = rs.update_bar(&bar("100")).unwrap(); // -1
        if let SignalValue::Scalar(s) = r {
            // Symmetric distribution -> skew should be close to 0
            assert!(s.abs() < dec!(0.1), "symmetric returns, skew near 0, got {s}");
        }
    }

    #[test]
    fn test_rsr_constant_returns_unavailable() {
        // Constant returns: std=0 -> Unavailable
        let mut rs = RollingSkewReturns::new("rs", 3).unwrap();
        rs.update_bar(&bar("100")).unwrap();
        rs.update_bar(&bar("101")).unwrap(); // +1
        rs.update_bar(&bar("102")).unwrap(); // +1
        let r = rs.update_bar(&bar("103")).unwrap(); // +1 -> std=0
        assert_eq!(r, SignalValue::Unavailable);
    }

    #[test]
    fn test_rsr_reset() {
        let mut rs = RollingSkewReturns::new("rs", 4).unwrap();
        for p in ["100", "101", "100", "101", "100"] { rs.update_bar(&bar(p)).unwrap(); }
        assert!(rs.is_ready());
        rs.reset();
        assert!(!rs.is_ready());
    }
}
