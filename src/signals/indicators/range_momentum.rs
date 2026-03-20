//! Range Momentum indicator -- rate of change in bar range (volatility momentum).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Momentum -- rate of change of the bar's range (high - low) over `period` bars.
///
/// ```text
/// range[t]   = high[t] - low[t]
/// momentum   = (range[t] - range[t - period]) / range[t - period] * 100
/// ```
///
/// Positive values indicate volatility expansion; negative values indicate contraction.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// or if the prior range is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeMomentum;
/// use fin_primitives::signals::Signal;
/// let rm = RangeMomentum::new("rm", 10).unwrap();
/// assert_eq!(rm.period(), 10);
/// ```
pub struct RangeMomentum {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl RangeMomentum {
    /// Constructs a new `RangeMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for RangeMomentum {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.window.push_back(range);
        if self.window.len() > self.period + 1 {
            self.window.pop_front();
        }
        if self.window.len() <= self.period { return Ok(SignalValue::Unavailable); }
        let prior = self.window[0];
        if prior.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar((range - prior) / prior * Decimal::ONE_HUNDRED))
    }

    fn reset(&mut self) {
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let mp = Price::new((hp.value() + lp.value()) / Decimal::TWO).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: mp, high: hp, low: lp, close: mp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rm_period_0_error() { assert!(RangeMomentum::new("rm", 0).is_err()); }

    #[test]
    fn test_rm_unavailable_before_period_plus_one() {
        let mut rm = RangeMomentum::new("rm", 3).unwrap();
        // period=3 needs 4 bars total; 3rd bar is still unavailable
        rm.update_bar(&bar("110", "90")).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        let v = rm.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rm_same_range_is_zero() {
        let mut rm = RangeMomentum::new("rm", 3).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        // 4th bar: range=20, prior(bar1)=20 -> 0%
        let v = rm.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rm_expansion_positive() {
        let mut rm = RangeMomentum::new("rm", 1).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();  // range=20
        // 2nd bar: range=40, prior=20 -> (40-20)/20*100 = 100%
        let v = rm.update_bar(&bar("120", "80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_rm_contraction_negative() {
        let mut rm = RangeMomentum::new("rm", 1).unwrap();
        rm.update_bar(&bar("120", "80")).unwrap();  // range=40
        // 2nd bar: range=20, prior=40 -> (20-40)/40*100 = -50%
        let v = rm.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-50)));
    }

    #[test]
    fn test_rm_zero_prior_range_unavailable() {
        let mut rm = RangeMomentum::new("rm", 1).unwrap();
        rm.update_bar(&bar("100", "100")).unwrap(); // range=0
        let v = rm.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rm_reset() {
        let mut rm = RangeMomentum::new("rm", 2).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        assert!(rm.is_ready());
        rm.reset();
        assert!(!rm.is_ready());
    }
}
