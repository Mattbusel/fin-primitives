//! Absolute Return Mean indicator -- rolling mean of absolute close-to-close returns.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Absolute Return Mean -- rolling average of |close[t] - close[t-1]|.
///
/// A volatility proxy that is scale-invariant and intuitive: it represents
/// the average absolute price move per bar over the period.
///
/// ```text
/// abs_ret[t] = |close[t] - close[t-1]|
/// mean[t]    = SMA(abs_ret, period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AbsReturnMean;
/// use fin_primitives::signals::Signal;
/// let arm = AbsReturnMean::new("arm", 14).unwrap();
/// assert_eq!(arm.period(), 14);
/// ```
pub struct AbsReturnMean {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl AbsReturnMean {
    /// Constructs a new `AbsReturnMean`.
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
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for AbsReturnMean {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let abs_ret = (bar.close - pc).abs();
            self.window.push_back(abs_ret);
            self.sum += abs_ret;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() { self.sum -= old; }
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(self.sum / Decimal::from(self.period as u32)))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
        self.sum = Decimal::ZERO;
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
    fn test_arm_period_0_error() { assert!(AbsReturnMean::new("arm", 0).is_err()); }

    #[test]
    fn test_arm_unavailable_before_period() {
        let mut arm = AbsReturnMean::new("arm", 3).unwrap();
        assert_eq!(arm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(arm.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_arm_flat_price_is_zero() {
        let mut arm = AbsReturnMean::new("arm", 3).unwrap();
        arm.update_bar(&bar("100")).unwrap();
        arm.update_bar(&bar("100")).unwrap();
        arm.update_bar(&bar("100")).unwrap();
        let v = arm.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_arm_constant_moves() {
        // Alternating +5/-5 -> abs_ret = 5 each -> mean = 5
        let mut arm = AbsReturnMean::new("arm", 3).unwrap();
        arm.update_bar(&bar("100")).unwrap();
        arm.update_bar(&bar("105")).unwrap(); // |+5|
        arm.update_bar(&bar("100")).unwrap(); // |-5|
        let v = arm.update_bar(&bar("105")).unwrap(); // |+5| -> mean([5,5,5]) = 5
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_arm_window_slides() {
        // period=2: [5,10] -> mean=7.5 then [10,3] -> mean=6.5
        let mut arm = AbsReturnMean::new("arm", 2).unwrap();
        arm.update_bar(&bar("100")).unwrap();
        arm.update_bar(&bar("105")).unwrap(); // |5|
        arm.update_bar(&bar("95")).unwrap();  // |10| -> mean([5,10]) = 7.5
        let v = arm.update_bar(&bar("98")).unwrap(); // |3| -> mean([10,3]) = 6.5
        assert_eq!(v, SignalValue::Scalar(dec!(6.5)));
    }

    #[test]
    fn test_arm_reset() {
        let mut arm = AbsReturnMean::new("arm", 3).unwrap();
        for p in ["100", "101", "102", "103"] { arm.update_bar(&bar(p)).unwrap(); }
        assert!(arm.is_ready());
        arm.reset();
        assert!(!arm.is_ready());
    }
}
