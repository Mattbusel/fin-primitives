//! Rolling Price Return Skewness indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Price Return Skewness — the skewness of close-to-close returns over
/// the last `period` bars.
///
/// ```text
/// ret[i]   = (close[i] - close[i-1]) / close[i-1]
/// skew     = (mean((ret - mean)^3)) / std^3
/// ```
///
/// - **Positive skew (> 0)**: there are more extreme positive returns (right-tail risk/reward).
/// - **Negative skew (< 0)**: there are more extreme negative returns (left-tail risk — common in equities).
/// - **Near zero**: roughly symmetric return distribution.
///
/// Returns [`SignalValue::Unavailable`] until `period` returns are collected
/// (`period + 1` closes), or when the standard deviation of returns is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 3`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceReturnSkew;
/// use fin_primitives::signals::Signal;
/// let prs = PriceReturnSkew::new("prs_20", 20).unwrap();
/// assert_eq!(prs.period(), 20);
/// ```
pub struct PriceReturnSkew {
    name: String,
    period: usize,
    returns: VecDeque<f64>,
    prev_close: Option<f64>,
}

impl PriceReturnSkew {
    /// Constructs a new `PriceReturnSkew`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 3`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 3 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            returns: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for PriceReturnSkew {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let c = bar.close.to_f64().unwrap_or(0.0);
        if let Some(pc) = self.prev_close {
            if pc > 0.0 {
                let ret = (c - pc) / pc;
                self.returns.push_back(ret);
                if self.returns.len() > self.period {
                    self.returns.pop_front();
                }
            }
        }
        self.prev_close = Some(c);

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.returns.len() as f64;
        let mean = self.returns.iter().sum::<f64>() / n;
        let variance = self.returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let skew = self.returns
            .iter()
            .map(|r| ((r - mean) / std_dev).powi(3))
            .sum::<f64>()
            / n;

        Decimal::try_from(skew)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn reset(&mut self) {
        self.returns.clear();
        self.prev_close = None;
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
    fn test_prs_invalid_period() {
        assert!(PriceReturnSkew::new("prs", 0).is_err());
        assert!(PriceReturnSkew::new("prs", 2).is_err());
    }

    #[test]
    fn test_prs_unavailable_during_warmup() {
        let mut prs = PriceReturnSkew::new("prs", 4).unwrap();
        for p in &["100", "101", "99", "102"] {
            assert_eq!(prs.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!prs.is_ready());
    }

    #[test]
    fn test_prs_flat_prices_unavailable() {
        // All same price → std_dev = 0 → Unavailable
        let mut prs = PriceReturnSkew::new("prs", 4).unwrap();
        for _ in 0..6 {
            prs.update_bar(&bar("100")).unwrap();
        }
        assert_eq!(prs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_prs_symmetric_near_zero() {
        // Small oscillations around a mean → near-zero skew
        let mut prs = PriceReturnSkew::new("prs", 5).unwrap();
        for p in &["100", "101", "99", "101", "99", "101"] {
            prs.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = prs.update_bar(&bar("100")).unwrap() {
            assert!(v.abs() < dec!(1), "symmetric returns → near-zero skew: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_prs_positive_outlier_positive_skew() {
        // One large positive jump, rest small → positive skew
        let mut prs = PriceReturnSkew::new("prs", 4).unwrap();
        prs.update_bar(&bar("100")).unwrap();
        prs.update_bar(&bar("100.1")).unwrap(); // tiny +0.1%
        prs.update_bar(&bar("100.2")).unwrap(); // tiny +0.1%
        prs.update_bar(&bar("100.3")).unwrap(); // tiny +0.1%
        if let SignalValue::Scalar(v) = prs.update_bar(&bar("110")).unwrap() { // big +9.7%
            assert!(v > dec!(0), "positive outlier → positive skew: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_prs_reset() {
        let mut prs = PriceReturnSkew::new("prs", 4).unwrap();
        for p in &["100","101","99","102","100"] { prs.update_bar(&bar(p)).unwrap(); }
        assert!(prs.is_ready());
        prs.reset();
        assert!(!prs.is_ready());
    }
}
