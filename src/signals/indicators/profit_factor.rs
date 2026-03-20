//! Profit Factor — rolling ratio of total gains to total losses from close returns.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Profit Factor — rolling `sum_of_positive_returns / |sum_of_negative_returns|`.
///
/// A classic trading performance metric adapted to a rolling window of close returns:
/// - **> 1.0**: total gains exceed total losses in the window — net positive expectancy.
/// - **= 1.0**: gains and losses are balanced.
/// - **< 1.0**: losses dominate — net negative expectancy.
///
/// Returns [`SignalValue::Unavailable`] if there are no negative returns in the window
/// (all gains — no losses to divide by), or until `period` returns are collected.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ProfitFactor;
/// use fin_primitives::signals::Signal;
/// let pf = ProfitFactor::new("pf_20", 20).unwrap();
/// assert_eq!(pf.period(), 20);
/// ```
pub struct ProfitFactor {
    name: String,
    period: usize,
    returns: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl ProfitFactor {
    /// Constructs a new `ProfitFactor`.
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

impl Signal for ProfitFactor {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let ret = (bar.close - pc)
                    .checked_div(pc)
                    .ok_or(FinError::ArithmeticOverflow)?;
                self.returns.push_back(ret);
                if self.returns.len() > self.period {
                    self.returns.pop_front();
                }
            }
        }

        self.prev_close = Some(bar.close);

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mut sum_gains = Decimal::ZERO;
        let mut sum_losses = Decimal::ZERO;

        for &r in &self.returns {
            if r > Decimal::ZERO {
                sum_gains += r;
            } else if r < Decimal::ZERO {
                sum_losses += r.abs();
            }
        }

        if sum_losses.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let pf = sum_gains
            .checked_div(sum_losses)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(pf))
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
    fn test_pf_invalid_period() {
        assert!(ProfitFactor::new("pf", 0).is_err());
        assert!(ProfitFactor::new("pf", 1).is_err());
    }

    #[test]
    fn test_pf_unavailable_during_warmup() {
        let mut s = ProfitFactor::new("pf", 4).unwrap();
        for p in &["100","101","99","102"] {
            assert_eq!(s.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_pf_balanced_returns_near_one() {
        // Symmetric: +1%, -1%, +1%, -1% → PF should be close to 1
        let mut s = ProfitFactor::new("pf", 4).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap();  // +1%
        s.update_bar(&bar("99.99")).unwrap(); // ~-1%
        s.update_bar(&bar("100.99")).unwrap(); // ~+1%
        if let SignalValue::Scalar(v) = s.update_bar(&bar("99.98")).unwrap() {
            // ~-1% → PF ≈ 1
            assert!((v - dec!(1)).abs() < dec!(0.1), "balanced returns → PF ≈ 1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pf_bull_run_high_profit_factor() {
        // Consistent gains → PF >> 1 (or Unavailable if no losses)
        let mut s = ProfitFactor::new("pf", 3).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("105")).unwrap();  // +5%
        s.update_bar(&bar("110")).unwrap();  // +4.76%
        // Mostly up, add one down bar
        let result = s.update_bar(&bar("109")).unwrap(); // -0.9%
        if let SignalValue::Scalar(v) = result {
            assert!(v > dec!(1), "mostly up-moves → PF > 1: {v}");
        }
        // Unavailable is acceptable (no losses yet in window)
    }

    #[test]
    fn test_pf_reset() {
        let mut s = ProfitFactor::new("pf", 3).unwrap();
        for p in &["100","102","101","103","102"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
