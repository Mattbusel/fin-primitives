//! Trend Bias Index indicator.
//!
//! Signal-to-noise ratio for returns: rolling mean return divided by the
//! rolling mean absolute deviation (MAD) of returns.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Bias Index — `mean(ret) / MAD(ret)` over a rolling window.
///
/// ```text
/// ret[i]  = (close[i] - close[i-1]) / close[i-1]
/// mean    = sum(ret) / n
/// MAD     = mean(|ret - mean|)
/// TBI     = mean / MAD
/// ```
///
/// Interpretation:
/// - **High positive**: strong upward trend, returns are consistently positive.
/// - **High negative**: strong downward trend, returns are consistently negative.
/// - **Near 0**: noisy / choppy market — directional signal is weak relative to variability.
///
/// Returns [`SignalValue::Unavailable`] until `period` returns are collected
/// (`period + 1` bars), or when MAD is zero (all returns identical).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendBiasIndex;
/// use fin_primitives::signals::Signal;
/// let tbi = TrendBiasIndex::new("tbi_20", 20).unwrap();
/// assert_eq!(tbi.period(), 20);
/// ```
pub struct TrendBiasIndex {
    name: String,
    period: usize,
    returns: VecDeque<f64>,
    prev_close: Option<f64>,
}

impl TrendBiasIndex {
    /// Constructs a new `TrendBiasIndex`.
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

impl Signal for TrendBiasIndex {
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
        let mad = self.returns.iter().map(|r| (r - mean).abs()).sum::<f64>() / n;

        if mad == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let tbi = mean / mad;
        Decimal::try_from(tbi)
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
    fn test_tbi_invalid_period() {
        assert!(TrendBiasIndex::new("tbi", 0).is_err());
        assert!(TrendBiasIndex::new("tbi", 1).is_err());
    }

    #[test]
    fn test_tbi_unavailable_during_warmup() {
        let mut tbi = TrendBiasIndex::new("tbi", 4).unwrap();
        for p in &["100", "101", "102", "103"] {
            assert_eq!(tbi.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!tbi.is_ready());
    }

    #[test]
    fn test_tbi_identical_returns_unavailable() {
        // Flat prices: all returns = 0 → mean = 0, MAD = 0 → Unavailable
        let mut tbi = TrendBiasIndex::new("tbi", 3).unwrap();
        // Feed period+1 bars to fill the window, then one more to trigger calculation
        for _ in 0..5 { tbi.update_bar(&bar("100")).unwrap(); }
        assert_eq!(tbi.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_tbi_uptrend_positive() {
        // Consistent up returns → positive TBI
        let mut tbi = TrendBiasIndex::new("tbi", 4).unwrap();
        // prices: 100, 102, 104, 108, 116 (accelerating up)
        for p in &["100", "102", "104", "108"] {
            tbi.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = tbi.update_bar(&bar("116")).unwrap() {
            assert!(v > dec!(0), "uptrend → positive TBI: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_tbi_downtrend_negative() {
        let mut tbi = TrendBiasIndex::new("tbi", 4).unwrap();
        for p in &["116", "112", "108", "104"] {
            tbi.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = tbi.update_bar(&bar("100")).unwrap() {
            assert!(v < dec!(0), "downtrend → negative TBI: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_tbi_reset() {
        let mut tbi = TrendBiasIndex::new("tbi", 3).unwrap();
        for p in &["100", "101", "102", "103"] { tbi.update_bar(&bar(p)).unwrap(); }
        assert!(tbi.is_ready());
        tbi.reset();
        assert!(!tbi.is_ready());
    }
}
