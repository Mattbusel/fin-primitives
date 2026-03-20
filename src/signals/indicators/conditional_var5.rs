//! Conditional Value at Risk 5% (CVaR / Expected Shortfall) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Conditional VaR 5% (CVaR95 / Expected Shortfall) -- the average close-to-close
/// return over the worst 5% of observations in the rolling window.
///
/// More conservative than VaR5: while VaR tells you the threshold, CVaR tells
/// you the expected loss *given* that the worst 5% scenario has occurred.
///
/// ```text
/// return[t]  = (close[t] - close[t-1]) / close[t-1] * 100
/// cvar5[t]   = mean of the bottom ceil(period * 0.05) returns
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ConditionalVar5;
/// use fin_primitives::signals::Signal;
/// let cv = ConditionalVar5::new("cvar5", 20).unwrap();
/// assert_eq!(cv.period(), 20);
/// ```
pub struct ConditionalVar5 {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
}

impl ConditionalVar5 {
    /// Constructs a new `ConditionalVar5`.
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

impl Signal for ConditionalVar5 {
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
        // Take bottom ceil(5%) of observations
        let tail_count = ((self.period as f64 * 0.05).ceil() as usize).max(1);
        let tail = &sorted[..tail_count.min(sorted.len())];
        let sum: Decimal = tail.iter().sum();
        #[allow(clippy::cast_possible_truncation)]
        let mean = sum / Decimal::from(tail.len() as u32);
        Ok(SignalValue::Scalar(mean))
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
    fn test_cvar5_period_0_error() { assert!(ConditionalVar5::new("cv", 0).is_err()); }

    #[test]
    fn test_cvar5_unavailable_before_period() {
        let mut cv = ConditionalVar5::new("cv", 5).unwrap();
        assert_eq!(cv.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cvar5_all_negative_returns() {
        // Falling prices -> all returns negative -> CVaR is the worst
        let mut cv = ConditionalVar5::new("cv", 4).unwrap();
        cv.update_bar(&bar("100")).unwrap();
        cv.update_bar(&bar("99")).unwrap();  // -1%
        cv.update_bar(&bar("98")).unwrap();  // ~-1.01%
        cv.update_bar(&bar("97")).unwrap();  // ~-1.02%
        let r = cv.update_bar(&bar("96")).unwrap(); // ~-1.03%
        if let SignalValue::Scalar(cvar) = r {
            assert!(cvar < dec!(0), "all negative returns, CVaR should be negative, got {cvar}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cvar5_big_loss_dominates() {
        // One huge loss should pull down CVaR
        let mut cv = ConditionalVar5::new("cv", 4).unwrap();
        cv.update_bar(&bar("100")).unwrap();
        cv.update_bar(&bar("50")).unwrap(); // -50%
        cv.update_bar(&bar("51")).unwrap();
        cv.update_bar(&bar("52")).unwrap();
        let r = cv.update_bar(&bar("53")).unwrap();
        if let SignalValue::Scalar(cvar) = r {
            // The -50% loss should be in the worst 5% tail, pulling CVaR negative
            assert!(cvar < dec!(0), "big loss present, CVaR should be negative, got {cvar}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cvar5_reset() {
        let mut cv = ConditionalVar5::new("cv", 3).unwrap();
        for p in ["100", "101", "102", "103"] { cv.update_bar(&bar(p)).unwrap(); }
        assert!(cv.is_ready());
        cv.reset();
        assert!(!cv.is_ready());
    }
}
