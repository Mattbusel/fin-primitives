//! Payoff Ratio — rolling average winning return divided by average losing return.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Payoff Ratio — rolling `mean_positive_return / |mean_negative_return|`.
///
/// Measures how large winning bars are on average compared to losing bars:
/// - **> 1.0**: average win is larger than average loss — favorable risk/reward.
/// - **= 1.0**: average win and loss are equal in magnitude.
/// - **< 1.0**: average loss is larger than average win — unfavorable risk/reward.
///
/// Complements win-rate metrics: a strategy can be profitable even with a low win rate
/// if the payoff ratio is high enough.
///
/// Returns [`SignalValue::Unavailable`] if the window has no negative or no positive returns.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PayoffRatio;
/// use fin_primitives::signals::Signal;
/// let pr = PayoffRatio::new("pr_20", 20).unwrap();
/// assert_eq!(pr.period(), 20);
/// ```
pub struct PayoffRatio {
    name: String,
    period: usize,
    returns: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl PayoffRatio {
    /// Constructs a new `PayoffRatio`.
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

impl Signal for PayoffRatio {
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

        let mut sum_pos = Decimal::ZERO;
        let mut count_pos: u32 = 0;
        let mut sum_neg = Decimal::ZERO;
        let mut count_neg: u32 = 0;

        for &r in &self.returns {
            if r > Decimal::ZERO {
                sum_pos += r;
                count_pos += 1;
            } else if r < Decimal::ZERO {
                sum_neg += r.abs();
                count_neg += 1;
            }
        }

        if count_pos == 0 || count_neg == 0 {
            return Ok(SignalValue::Unavailable);
        }

        let mean_pos = sum_pos
            .checked_div(Decimal::from(count_pos))
            .ok_or(FinError::ArithmeticOverflow)?;
        let mean_neg = sum_neg
            .checked_div(Decimal::from(count_neg))
            .ok_or(FinError::ArithmeticOverflow)?;

        let ratio = mean_pos
            .checked_div(mean_neg)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio))
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
    fn test_pr_invalid_period() {
        assert!(PayoffRatio::new("pr", 0).is_err());
        assert!(PayoffRatio::new("pr", 1).is_err());
    }

    #[test]
    fn test_pr_unavailable_during_warmup() {
        let mut s = PayoffRatio::new("pr", 4).unwrap();
        for p in &["100","102","99","103"] {
            assert_eq!(s.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_pr_symmetric_returns_near_one() {
        // +2% gain, -2% loss, +2% gain, -2% loss → payoff ratio ≈ 1
        let mut s = PayoffRatio::new("pr", 4).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("102")).unwrap();    // +2%
        s.update_bar(&bar("99.96")).unwrap();  // -2%
        s.update_bar(&bar("101.96")).unwrap(); // +2%
        if let SignalValue::Scalar(v) = s.update_bar(&bar("99.92")).unwrap() {
            // -2% → payoff ≈ 1
            assert!((v - dec!(1)).abs() < dec!(0.1), "symmetric returns → payoff ≈ 1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pr_large_wins_small_losses() {
        // Big wins, small losses → payoff > 1
        let mut s = PayoffRatio::new("pr", 4).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("110")).unwrap();   // +10%
        s.update_bar(&bar("109")).unwrap();   // -0.9%
        s.update_bar(&bar("120")).unwrap();   // +10.1%
        if let SignalValue::Scalar(v) = s.update_bar(&bar("119")).unwrap() {
            // -0.83% → payoff = avg_gain/avg_loss >> 1
            assert!(v > dec!(5), "large wins, small losses → payoff >> 1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pr_reset() {
        let mut s = PayoffRatio::new("pr", 4).unwrap();
        for p in &["100","102","101","103","100","104"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
