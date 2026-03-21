//! Rolling Sharpe Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Sharpe Ratio — mean close-to-close return divided by the standard deviation
/// of those returns over the last `period` bars.
///
/// ```text
/// ret[i]  = (close[i] - close[i-1]) / close[i-1]
/// mean    = sum(ret) / period
/// std_dev = sqrt(variance(ret))
/// sharpe  = mean / std_dev
/// ```
///
/// - **Positive and large**: returns are consistently positive relative to their volatility.
/// - **Near zero**: returns are noisy with no clear directional edge.
/// - **Negative**: mean return is negative.
///
/// Returns [`SignalValue::Unavailable`] until `period` returns are collected (needs
/// `period + 1` close prices), or when the standard deviation of returns is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SharpeRatio;
/// use fin_primitives::signals::Signal;
/// let sr = SharpeRatio::new("sr_20", 20).unwrap();
/// assert_eq!(sr.period(), 20);
/// ```
pub struct SharpeRatio {
    name: String,
    period: usize,
    returns: VecDeque<f64>,
    prev_close: Option<f64>,
}

impl SharpeRatio {
    /// Constructs a new `SharpeRatio`.
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
            returns: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for SharpeRatio {
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

        let sharpe = mean / std_dev;
        Decimal::try_from(sharpe)
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
    fn test_sr_invalid_period() {
        assert!(SharpeRatio::new("sr", 0).is_err());
        assert!(SharpeRatio::new("sr", 1).is_err());
    }

    #[test]
    fn test_sr_unavailable_during_warmup() {
        let mut sr = SharpeRatio::new("sr", 4).unwrap();
        for p in &["100", "101", "99", "102"] {
            assert_eq!(sr.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!sr.is_ready());
    }

    #[test]
    fn test_sr_flat_prices_unavailable() {
        // Same price every bar → std_dev = 0 → Unavailable
        let mut sr = SharpeRatio::new("sr", 3).unwrap();
        sr.update_bar(&bar("100")).unwrap();
        sr.update_bar(&bar("100")).unwrap();
        sr.update_bar(&bar("100")).unwrap();
        let v = sr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_sr_uptrend_positive() {
        // Consistent gains → positive Sharpe
        let mut sr = SharpeRatio::new("sr", 4).unwrap();
        for p in &["100", "102", "104", "106", "108"] {
            sr.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = sr.update_bar(&bar("110")).unwrap() {
            assert!(v > dec!(0), "uptrend should yield positive Sharpe: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sr_downtrend_negative() {
        // Consistent losses → negative Sharpe
        let mut sr = SharpeRatio::new("sr", 4).unwrap();
        for p in &["110", "108", "106", "104", "102"] {
            sr.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = sr.update_bar(&bar("100")).unwrap() {
            assert!(v < dec!(0), "downtrend should yield negative Sharpe: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sr_reset() {
        let mut sr = SharpeRatio::new("sr", 3).unwrap();
        for p in &["100", "102", "101", "103"] {
            sr.update_bar(&bar(p)).unwrap();
        }
        assert!(sr.is_ready());
        sr.reset();
        assert!(!sr.is_ready());
        assert!(sr.update_bar(&bar("100")).unwrap() == SignalValue::Unavailable);
    }
}
