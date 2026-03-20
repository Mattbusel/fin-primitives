//! Rolling Skewness indicator -- skewness of close-to-close returns over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Skewness -- measures the asymmetry of the return distribution over a
/// rolling `period`-bar window.
///
/// Uses the sample skewness formula:
/// ```text
/// skew = (n / ((n-1)(n-2))) * sum((r_i - mean)^3) / stddev^3
/// ```
///
/// Positive skewness indicates a right tail (occasional large gains);
/// negative skewness indicates a left tail (occasional large losses).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (needs `period` returns from `period + 1` closes) or when stddev is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingSkewness;
/// use fin_primitives::signals::Signal;
/// let rs = RollingSkewness::new("rs", 20).unwrap();
/// assert_eq!(rs.period(), 20);
/// ```
pub struct RollingSkewness {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl RollingSkewness {
    /// Constructs a new `RollingSkewness`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 3`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 3 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 2),
        })
    }
}

impl Signal for RollingSkewness {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 { self.closes.pop_front(); }
        if self.closes.len() <= self.period { return Ok(SignalValue::Unavailable); }

        use rust_decimal::prelude::ToPrimitive;
        let prices: Vec<f64> = self.closes.iter()
            .map(|d| d.to_f64().unwrap_or(f64::NAN))
            .collect();
        let returns: Vec<f64> = prices.windows(2).map(|w| w[1] - w[0]).collect();
        let n = returns.len() as f64;
        if n < 3.0 { return Ok(SignalValue::Unavailable); }

        let mean = returns.iter().sum::<f64>() / n;
        let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
        if variance <= 0.0 { return Ok(SignalValue::Unavailable); }
        let stddev = variance.sqrt();

        let m3 = returns.iter().map(|r| (r - mean).powi(3)).sum::<f64>();
        let skew = (n / ((n - 1.0) * (n - 2.0))) * m3 / stddev.powi(3);
        match Decimal::try_from(skew) {
            Ok(d) => Ok(SignalValue::Scalar(d)),
            Err(_) => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) { self.closes.clear(); }
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
    fn test_rs_period_too_small() { assert!(RollingSkewness::new("rs", 2).is_err()); }

    #[test]
    fn test_rs_unavailable_before_warmup() {
        let mut rs = RollingSkewness::new("rs", 5).unwrap();
        for _ in 0..5 {
            assert_eq!(rs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rs_symmetric_near_zero() {
        let mut rs = RollingSkewness::new("rs", 5).unwrap();
        // returns: +1,-1,+1,-1,+1 -> approx 0 skew
        let prices = ["100","101","100","101","100","101"];
        let mut last = SignalValue::Unavailable;
        for p in &prices { last = rs.update_bar(&bar(p)).unwrap(); }
        if let SignalValue::Scalar(s) = last {
            assert!(s.abs() < dec!(1), "expected near-zero skew for symmetric, got {s}");
        }
    }

    #[test]
    fn test_rs_reset() {
        let mut rs = RollingSkewness::new("rs", 5).unwrap();
        for i in 0u32..8 { rs.update_bar(&bar(&(100+i).to_string())).unwrap(); }
        assert!(rs.is_ready());
        rs.reset();
        assert!(!rs.is_ready());
    }
}
