//! Log Return Volatility — standard deviation of log returns over a rolling window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Log Return Volatility — rolling standard deviation of log returns.
///
/// Computes the sample standard deviation of `ln(close[t] / close[t-1])` over
/// the last `period` bars. This is the most common realized volatility estimator
/// used in options pricing and risk management.
///
/// The result is expressed in the same units as the log returns (per-bar). To
/// annualize, multiply by `sqrt(bars_per_year)` (e.g. `sqrt(252)` for daily bars).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (one extra bar for the first log return) or when variance is negative (floating-point edge case).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LogReturnVolatility;
/// use fin_primitives::signals::Signal;
/// let lrv = LogReturnVolatility::new("lrv_20", 20).unwrap();
/// assert_eq!(lrv.period(), 20);
/// ```
pub struct LogReturnVolatility {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    log_returns: VecDeque<f64>,
}

impl LogReturnVolatility {
    /// Constructs a new `LogReturnVolatility`.
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
            prev_close: None,
            log_returns: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for LogReturnVolatility {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.log_returns.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let Some(prev) = self.prev_close else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };
        self.prev_close = Some(bar.close);

        if prev <= Decimal::ZERO {
            return Ok(SignalValue::Unavailable);
        }

        let curr_f = bar.close.to_f64().unwrap_or(0.0);
        let prev_f = prev.to_f64().unwrap_or(0.0);

        if prev_f <= 0.0 || curr_f <= 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let log_ret = (curr_f / prev_f).ln();
        self.log_returns.push_back(log_ret);
        if self.log_returns.len() > self.period {
            self.log_returns.pop_front();
        }

        if self.log_returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.log_returns.len() as f64;
        let mean = self.log_returns.iter().sum::<f64>() / n;
        let variance = self.log_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);

        if variance < 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let std_dev = variance.sqrt();
        let result = Decimal::try_from(std_dev).unwrap_or(Decimal::ZERO);
        Ok(SignalValue::Scalar(result))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.log_returns.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_lrv_invalid_period() {
        assert!(LogReturnVolatility::new("lrv", 0).is_err());
        assert!(LogReturnVolatility::new("lrv", 1).is_err());
    }

    #[test]
    fn test_lrv_unavailable_before_period() {
        let mut lrv = LogReturnVolatility::new("lrv", 3).unwrap();
        for p in &["100", "101", "102"] {
            let v = lrv.update_bar(&bar(p)).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
        assert!(!lrv.is_ready());
    }

    #[test]
    fn test_lrv_constant_prices_zero_volatility() {
        let mut lrv = LogReturnVolatility::new("lrv", 3).unwrap();
        for _ in 0..5 {
            lrv.update_bar(&bar("100")).unwrap();
        }
        // All log returns = ln(1) = 0, std_dev = 0.
        if let SignalValue::Scalar(v) = lrv.update_bar(&bar("100")).unwrap() {
            assert!(v.abs() < dec!(0.000001), "expected ~0 volatility, got {v}");
        }
    }

    #[test]
    fn test_lrv_non_negative() {
        let mut lrv = LogReturnVolatility::new("lrv", 5).unwrap();
        let prices = ["100", "102", "101", "103", "102", "104", "103"];
        for p in &prices {
            if let SignalValue::Scalar(v) = lrv.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0), "volatility should be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_lrv_reset() {
        let mut lrv = LogReturnVolatility::new("lrv", 3).unwrap();
        for p in &["100", "101", "102", "103", "104"] {
            lrv.update_bar(&bar(p)).unwrap();
        }
        assert!(lrv.is_ready());
        lrv.reset();
        assert!(!lrv.is_ready());
    }

    #[test]
    fn test_lrv_period_and_name() {
        let lrv = LogReturnVolatility::new("my_lrv", 20).unwrap();
        assert_eq!(lrv.period(), 20);
        assert_eq!(lrv.name(), "my_lrv");
    }
}
