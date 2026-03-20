//! Daily Return Skew indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Daily Return Skew — the skewness of close-to-close returns over `period` bars.
///
/// Positive skew indicates a tail toward large positive returns; negative skew
/// indicates a tail toward large negative returns.
///
/// Uses the population skewness formula: `E[(X-μ)³] / σ³`.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// or if std dev is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DailyReturnSkew;
/// use fin_primitives::signals::Signal;
///
/// let drs = DailyReturnSkew::new("drs", 20).unwrap();
/// assert_eq!(drs.period(), 20);
/// ```
pub struct DailyReturnSkew {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<f64>,
}

impl DailyReturnSkew {
    /// Constructs a new `DailyReturnSkew`.
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
            prev_close: None,
            returns: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for DailyReturnSkew {
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
                    let var = self.returns.iter().map(|r| { let d = r - mean; d * d }).sum::<f64>() / n;
                    let std_dev = var.sqrt();
                    if std_dev == 0.0 {
                        return Ok(SignalValue::Unavailable);
                    }
                    let skew = self.returns.iter()
                        .map(|r| { let d = (r - mean) / std_dev; d * d * d })
                        .sum::<f64>() / n;
                    match Decimal::from_f64(skew) {
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
    fn test_drs_invalid_period() {
        assert!(DailyReturnSkew::new("drs", 0).is_err());
        assert!(DailyReturnSkew::new("drs", 2).is_err());
    }

    #[test]
    fn test_drs_unavailable_before_warm_up() {
        let mut drs = DailyReturnSkew::new("drs", 5).unwrap();
        for _ in 0..5 {
            assert_eq!(drs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_drs_symmetric_returns_near_zero() {
        // Returns that are symmetric: -1, +1, -1, +1, -1, +1...
        // Skewness of a symmetric distribution should be near 0
        let mut drs = DailyReturnSkew::new("drs", 4).unwrap();
        let prices = ["100", "101", "100", "101", "100", "101"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = drs.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v.abs() < dec!(0.01), "symmetric returns should give near-zero skew: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_drs_positive_spike_gives_positive_skew() {
        // Long string of small negatives then one large positive → positive skew
        let mut drs = DailyReturnSkew::new("drs", 5).unwrap();
        let prices = ["100", "99", "98", "97", "96", "120"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = drs.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "spike should give positive skew: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_drs_reset() {
        let mut drs = DailyReturnSkew::new("drs", 4).unwrap();
        for p in ["100", "101", "100", "101", "100"] { drs.update_bar(&bar(p)).unwrap(); }
        assert!(drs.is_ready());
        drs.reset();
        assert!(!drs.is_ready());
    }
}
