//! Return Mean Deviation indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Return Mean Deviation — the mean absolute deviation (MAD) of close-to-close
/// returns over `period` bars.
///
/// ```text
/// returns = [(close[t] - close[t-1]) / close[t-1]] for each bar
/// MAD     = mean(|return - mean(returns)|)
/// ```
///
/// Unlike standard deviation, MAD is more robust to outliers and useful for
/// measuring typical return variability.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ReturnMeanDeviation;
/// use fin_primitives::signals::Signal;
///
/// let rmd = ReturnMeanDeviation::new("rmd", 20).unwrap();
/// assert_eq!(rmd.period(), 20);
/// ```
pub struct ReturnMeanDeviation {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<f64>,
}

impl ReturnMeanDeviation {
    /// Constructs a new `ReturnMeanDeviation`.
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
            returns: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for ReturnMeanDeviation {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                let ret = match (bar.close.to_f64(), pc.to_f64()) {
                    (Some(c), Some(p)) if p != 0.0 => (c - p) / p,
                    _ => return Ok(SignalValue::Unavailable),
                };

                self.returns.push_back(ret);
                if self.returns.len() > self.period { self.returns.pop_front(); }

                if self.returns.len() < self.period {
                    SignalValue::Unavailable
                } else {
                    let n = self.returns.len() as f64;
                    let mean = self.returns.iter().sum::<f64>() / n;
                    let mad = self.returns.iter().map(|r| (r - mean).abs()).sum::<f64>() / n;
                    match Decimal::from_f64(mad) {
                        Some(v) => SignalValue::Scalar(v),
                        None => SignalValue::Unavailable,
                    }
                }
            }
        };
        self.prev_close = Some(bar.close);
        Ok(result)
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
    fn test_rmd_invalid_period() {
        assert!(ReturnMeanDeviation::new("rmd", 0).is_err());
        assert!(ReturnMeanDeviation::new("rmd", 1).is_err());
    }

    #[test]
    fn test_rmd_unavailable_before_warm_up() {
        let mut rmd = ReturnMeanDeviation::new("rmd", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(rmd.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_rmd_constant_returns_gives_zero() {
        // All bars have the same return → MAD = 0
        let mut rmd = ReturnMeanDeviation::new("rmd", 3).unwrap();
        let prices = ["100", "101", "102.01", "103.0301", "104.060401"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = rmd.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(0.001), "constant returns should give near-zero MAD: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rmd_varying_returns_positive() {
        let mut rmd = ReturnMeanDeviation::new("rmd", 3).unwrap();
        let prices = ["100", "110", "100", "115", "95"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = rmd.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "varying returns should give positive MAD: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rmd_reset() {
        let mut rmd = ReturnMeanDeviation::new("rmd", 3).unwrap();
        for p in ["100", "101", "102", "103"] { rmd.update_bar(&bar(p)).unwrap(); }
        assert!(rmd.is_ready());
        rmd.reset();
        assert!(!rmd.is_ready());
    }
}
