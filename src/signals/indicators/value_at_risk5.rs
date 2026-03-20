//! Value at Risk 5% indicator -- 5th-percentile rolling close return.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Value at Risk 5% (VaR95) -- the 5th-percentile close-to-close return over
/// the last `period` bars, expressed as a percentage.
///
/// Interpretation: with 95% confidence, the one-bar loss will not exceed
/// the absolute value of this number (a negative value represents a loss).
///
/// ```text
/// return[t]  = (close[t] - close[t-1]) / close[t-1] * 100
/// var5pct[t] = percentile_5(returns, period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ValueAtRisk5;
/// use fin_primitives::signals::Signal;
/// let var = ValueAtRisk5::new("var5", 20).unwrap();
/// assert_eq!(var.period(), 20);
/// ```
pub struct ValueAtRisk5 {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
}

impl ValueAtRisk5 {
    /// Constructs a new `ValueAtRisk5`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for ValueAtRisk5 {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc) / pc * Decimal::ONE_HUNDRED;
                self.window.push_back(ret);
                if self.window.len() > self.period {
                    self.window.pop_front();
                }
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        let mut sorted: Vec<Decimal> = self.window.iter().copied().collect();
        sorted.sort();
        // 5th percentile index
        let idx = (self.period as f64 * 0.05) as usize;
        let idx = idx.min(sorted.len() - 1);
        Ok(SignalValue::Scalar(sorted[idx]))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
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
    fn test_var5_period_0_error() { assert!(ValueAtRisk5::new("v", 0).is_err()); }

    #[test]
    fn test_var5_unavailable_before_period() {
        let mut v = ValueAtRisk5::new("v", 5).unwrap();
        assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(v.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_var5_all_positive_returns() {
        // All returns +1%, worst 5th percentile is still +1%
        let mut v = ValueAtRisk5::new("v", 4).unwrap();
        v.update_bar(&bar("100")).unwrap();
        v.update_bar(&bar("101")).unwrap();
        v.update_bar(&bar("102")).unwrap();
        v.update_bar(&bar("103")).unwrap();
        let r = v.update_bar(&bar("104")).unwrap();
        if let SignalValue::Scalar(var) = r {
            // All returns are positive ~1%, VaR5 >= 0
            assert!(var > dec!(0), "All positive returns, VaR5 should be > 0, got {var}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_var5_includes_big_loss() {
        // One big loss should pull down the 5th percentile
        let mut v = ValueAtRisk5::new("v", 5).unwrap();
        v.update_bar(&bar("100")).unwrap();
        v.update_bar(&bar("50")).unwrap();  // -50% return
        v.update_bar(&bar("51")).unwrap();
        v.update_bar(&bar("52")).unwrap();
        v.update_bar(&bar("53")).unwrap();
        let r = v.update_bar(&bar("54")).unwrap();
        if let SignalValue::Scalar(var) = r {
            // Sorted returns have the -50% as the smallest
            assert!(var < dec!(0), "Should have negative VaR5, got {var}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_var5_reset() {
        let mut v = ValueAtRisk5::new("v", 3).unwrap();
        for p in ["100", "101", "102", "103"] { v.update_bar(&bar(p)).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
