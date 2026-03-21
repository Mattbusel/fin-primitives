//! Rolling Sortino Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Sortino Ratio — mean close-to-close return divided by the downside deviation
/// (standard deviation of negative returns only) over the last `period` bars.
///
/// ```text
/// ret[i]        = (close[i] - close[i-1]) / close[i-1]
/// mean          = sum(ret) / period
/// neg_returns   = {r in ret : r < 0}
/// downside_var  = sum(r^2 for r in neg_returns) / period
/// downside_dev  = sqrt(downside_var)
/// sortino       = mean / downside_dev
/// ```
///
/// Unlike the Sharpe ratio, the Sortino ratio penalizes only downside volatility,
/// making it more appropriate for return distributions with positive skew.
///
/// Returns [`SignalValue::Unavailable`] until `period` returns are collected, or
/// when there are no negative returns in the window (no downside to divide by).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SortinoRatio;
/// use fin_primitives::signals::Signal;
/// let sor = SortinoRatio::new("sor_20", 20).unwrap();
/// assert_eq!(sor.period(), 20);
/// ```
pub struct SortinoRatio {
    name: String,
    period: usize,
    returns: VecDeque<f64>,
    prev_close: Option<f64>,
}

impl SortinoRatio {
    /// Constructs a new `SortinoRatio`.
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

impl Signal for SortinoRatio {
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

        // Downside variance uses all negative returns squared, divided by period
        let downside_var = self.returns
            .iter()
            .filter(|&&r| r < 0.0)
            .map(|r| r * r)
            .sum::<f64>()
            / n;

        let downside_dev = downside_var.sqrt();
        if downside_dev == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let sortino = mean / downside_dev;
        Decimal::try_from(sortino)
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
    fn test_sor_invalid_period() {
        assert!(SortinoRatio::new("sor", 0).is_err());
        assert!(SortinoRatio::new("sor", 1).is_err());
    }

    #[test]
    fn test_sor_unavailable_during_warmup() {
        let mut sor = SortinoRatio::new("sor", 4).unwrap();
        for p in &["100", "101", "99", "102"] {
            assert_eq!(sor.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!sor.is_ready());
    }

    #[test]
    fn test_sor_only_gains_unavailable() {
        // All upward moves → no negative returns → Unavailable (no downside)
        let mut sor = SortinoRatio::new("sor", 3).unwrap();
        for p in &["100", "102", "104", "106"] {
            sor.update_bar(&bar(p)).unwrap();
        }
        assert_eq!(sor.update_bar(&bar("108")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_sor_uptrend_with_pullback_positive() {
        // Strong uptrend with one small pullback → positive Sortino
        let mut sor = SortinoRatio::new("sor", 4).unwrap();
        for p in &["100", "104", "108", "106", "112"] {
            sor.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = sor.update_bar(&bar("116")).unwrap() {
            assert!(v > dec!(0), "net uptrend with low downside → positive Sortino: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sor_downtrend_negative() {
        // Consistent losses → negative mean → negative Sortino
        let mut sor = SortinoRatio::new("sor", 4).unwrap();
        for p in &["110", "108", "106", "104", "102"] {
            sor.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = sor.update_bar(&bar("100")).unwrap() {
            assert!(v < dec!(0), "downtrend should yield negative Sortino: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_sor_reset() {
        let mut sor = SortinoRatio::new("sor", 3).unwrap();
        for p in &["100", "102", "99", "103"] {
            sor.update_bar(&bar(p)).unwrap();
        }
        assert!(sor.is_ready());
        sor.reset();
        assert!(!sor.is_ready());
        assert_eq!(sor.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
