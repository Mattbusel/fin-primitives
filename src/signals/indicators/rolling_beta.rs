//! Rolling Beta indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Beta — sensitivity of the bar's close return to its own lagged return.
///
/// This is an auto-beta (serial sensitivity): it measures how much the current
/// period's return is predicted by the prior period's return using a rolling OLS:
///
/// ```text
/// ret[i]    = (close[i] - close[i-1]) / close[i-1]
/// beta      = Cov(ret[t-period+1..t], ret[t-period..t-1]) / Var(ret[t-period..t-1])
/// ```
///
/// - **Beta > 0**: positive autocorrelation — trends tend to continue (momentum regime).
/// - **Beta < 0**: negative autocorrelation — returns tend to mean-revert.
/// - **Beta ≈ 0**: returns are serially uncorrelated (efficient market assumption).
///
/// Returns [`SignalValue::Unavailable`] until `period + 2` closes are collected, or
/// when the variance of lagged returns is zero (all returns identical).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingBeta;
/// use fin_primitives::signals::Signal;
/// let rb = RollingBeta::new("beta_20", 20).unwrap();
/// assert_eq!(rb.period(), 20);
/// ```
pub struct RollingBeta {
    name: String,
    period: usize,
    closes: VecDeque<f64>,
}

impl RollingBeta {
    /// Constructs a new `RollingBeta`.
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
            closes: VecDeque::with_capacity(period + 2),
        })
    }
}

impl Signal for RollingBeta {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period + 1 }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let c = bar.close.to_f64().unwrap_or(0.0);
        self.closes.push_back(c);
        if self.closes.len() > self.period + 2 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 2 {
            return Ok(SignalValue::Unavailable);
        }

        // Build consecutive return pairs: (x[i], y[i]) where y[i] = x[i+1]
        let closes: Vec<f64> = self.closes.iter().copied().collect();
        let rets: Vec<f64> = closes
            .windows(2)
            .filter_map(|w| {
                if w[0] != 0.0 { Some((w[1] - w[0]) / w[0]) } else { None }
            })
            .collect();

        if rets.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        // x = lagged returns, y = current returns (offset by 1)
        let x = &rets[..self.period];
        let y = &rets[1..self.period + 1];

        let n = x.len() as f64;
        let mean_x = x.iter().sum::<f64>() / n;
        let mean_y = y.iter().sum::<f64>() / n;

        let cov = x.iter().zip(y.iter())
            .map(|(&xi, &yi)| (xi - mean_x) * (yi - mean_y))
            .sum::<f64>()
            / n;

        let var_x = x.iter()
            .map(|&xi| (xi - mean_x).powi(2))
            .sum::<f64>()
            / n;

        if var_x == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let beta = cov / var_x;
        Decimal::try_from(beta)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
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
    fn test_rb_invalid_period() {
        assert!(RollingBeta::new("rb", 0).is_err());
        assert!(RollingBeta::new("rb", 1).is_err());
    }

    #[test]
    fn test_rb_unavailable_during_warmup() {
        let mut rb = RollingBeta::new("rb", 3).unwrap();
        for p in &["100", "101", "102", "103"] {
            assert_eq!(rb.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!rb.is_ready());
    }

    #[test]
    fn test_rb_alternating_mean_reverts_negative() {
        // Up/down/up/down pattern → negative autocorrelation → negative beta
        let mut rb = RollingBeta::new("rb", 3).unwrap();
        let prices = ["100", "102", "100", "102", "100", "102"];
        let mut last = SignalValue::Unavailable;
        for &p in &prices {
            last = rb.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(0), "alternating returns → negative beta: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rb_trending_positive() {
        // Accelerating uptrend → positive autocorrelation → positive beta
        let mut rb = RollingBeta::new("rb", 3).unwrap();
        let prices = ["100", "102", "105", "109", "114", "120"];
        let mut last = SignalValue::Unavailable;
        for &p in &prices {
            last = rb.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "accelerating trend → positive beta: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rb_reset() {
        let mut rb = RollingBeta::new("rb", 3).unwrap();
        for p in &["100", "101", "102", "103", "104"] { rb.update_bar(&bar(p)).unwrap(); }
        assert!(rb.is_ready());
        rb.reset();
        assert!(!rb.is_ready());
    }
}
