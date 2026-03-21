//! Volatility-Normalized Return indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volatility-Normalized Return (per-bar Sharpe ratio proxy).
///
/// Computes the current bar's close-to-close return divided by the rolling
/// standard deviation of returns over the lookback window. This gives a
/// dimensionless per-bar signal that adjusts raw momentum for prevailing
/// volatility — high values indicate momentum is large relative to recent noise.
///
/// Formula:
/// - `r_t = (close_t − close_{t−1}) / close_{t−1}`
/// - `σ = stddev(r_{t−period+1..t})`
/// - `output = r_t / σ`   (zero when σ = 0)
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolNormalizedReturn;
/// use fin_primitives::signals::Signal;
/// let vnr = VolNormalizedReturn::new("vnr_20", 20).unwrap();
/// assert_eq!(vnr.period(), 20);
/// ```
pub struct VolNormalizedReturn {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<f64>,
}

impl VolNormalizedReturn {
    /// Constructs a new `VolNormalizedReturn` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            returns: VecDeque::with_capacity(period),
        })
    }

    fn stddev(data: &VecDeque<f64>) -> f64 {
        let n = data.len() as f64;
        if n < 2.0 {
            return 0.0;
        }
        let mean = data.iter().sum::<f64>() / n;
        let var = data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
        var.sqrt()
    }
}

impl Signal for VolNormalizedReturn {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let Some(pc) = self.prev_close else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };

        let current_ret = if pc.is_zero() {
            0.0
        } else {
            let ret = (bar.close - pc)
                .checked_div(pc)
                .ok_or(FinError::ArithmeticOverflow)?;
            ret.to_f64().unwrap_or(0.0)
        };

        self.prev_close = Some(bar.close);
        self.returns.push_back(current_ret);
        if self.returns.len() > self.period {
            self.returns.pop_front();
        }

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sigma = Self::stddev(&self.returns);
        let normalized = if sigma == 0.0 { 0.0 } else { current_ret / sigma };
        Decimal::try_from(normalized)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn is_ready(&self) -> bool {
        self.returns.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.returns.clear();
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
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(
            VolNormalizedReturn::new("vnr", 0),
            Err(FinError::InvalidPeriod(0))
        ));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut vnr = VolNormalizedReturn::new("vnr", 3).unwrap();
        let v = vnr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_ready_after_period_plus_one() {
        let mut vnr = VolNormalizedReturn::new("vnr", 3).unwrap();
        // First bar sets prev_close; then need 3 returns
        for _ in 0..4 {
            vnr.update_bar(&bar("100")).unwrap();
        }
        assert!(vnr.is_ready());
    }

    #[test]
    fn test_constant_price_returns_zero() {
        let mut vnr = VolNormalizedReturn::new("vnr", 3).unwrap();
        for _ in 0..5 {
            vnr.update_bar(&bar("100")).unwrap();
        }
        let v = vnr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_large_return_gives_high_score() {
        // Alternate small moves then a large spike
        let mut vnr = VolNormalizedReturn::new("vnr", 5).unwrap();
        for p in ["100", "101", "100", "101", "100", "101"] {
            vnr.update_bar(&bar(p)).unwrap();
        }
        // Now a large positive return
        let v = vnr.update_bar(&bar("200")).unwrap();
        if let SignalValue::Scalar(s) = v {
            // Normalized return should be large and positive
            assert!(s > dec!(1));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset_clears_state() {
        let mut vnr = VolNormalizedReturn::new("vnr", 3).unwrap();
        for _ in 0..5 {
            vnr.update_bar(&bar("100")).unwrap();
        }
        assert!(vnr.is_ready());
        vnr.reset();
        assert!(!vnr.is_ready());
        assert!(vnr.prev_close.is_none());
    }
}
