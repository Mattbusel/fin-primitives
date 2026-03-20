//! Rolling Shadow Balance indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Shadow Balance — rolling average of per-bar shadow imbalance.
///
/// For each bar:
/// ```text
/// shadow_imbalance = (upper_shadow - lower_shadow) / range
/// ```
/// where `upper_shadow = high - max(open,close)` and `lower_shadow = min(open,close) - low`.
///
/// The indicator outputs the simple moving average of this per-bar imbalance over `period` bars.
///
/// - **Positive**: sustained upper-shadow bias — sellers consistently pushing price down from highs.
/// - **Negative**: sustained lower-shadow bias — buyers consistently supporting from lows.
/// - **Near zero**: balanced shadow structure.
/// - Bars with no range contribute `0` to the average.
/// - Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingShadowBalance;
/// use fin_primitives::signals::Signal;
///
/// let rsb = RollingShadowBalance::new("rsb", 10).unwrap();
/// assert_eq!(rsb.period(), 10);
/// ```
pub struct RollingShadowBalance {
    name: String,
    period: usize,
    values: VecDeque<Decimal>,
    sum: Decimal,
}

impl RollingShadowBalance {
    /// Constructs a new `RollingShadowBalance`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            values: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RollingShadowBalance {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.values.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let imbalance = if range.is_zero() {
            Decimal::ZERO
        } else {
            let upper = bar.high - bar.body_high();
            let lower = bar.body_low() - bar.low;
            (upper - lower) / range
        };

        self.sum += imbalance;
        self.values.push_back(imbalance);
        if self.values.len() > self.period {
            let removed = self.values.pop_front().unwrap();
            self.sum -= removed;
        }

        if self.values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(avg))
    }

    fn reset(&mut self) {
        self.values.clear();
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

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rsb_invalid_period() {
        assert!(RollingShadowBalance::new("rsb", 0).is_err());
    }

    #[test]
    fn test_rsb_unavailable_during_warmup() {
        let mut rsb = RollingShadowBalance::new("rsb", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(rsb.update_bar(&bar("100", "110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!rsb.is_ready());
    }

    #[test]
    fn test_rsb_all_upper_shadow_positive() {
        // open=close=100, high=110, low=100 → upper shadow dominates → positive
        let mut rsb = RollingShadowBalance::new("rsb", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = rsb.update_bar(&bar("100", "110", "100", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "upper shadow bias should be positive: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rsb_all_lower_shadow_negative() {
        // open=close=100, high=100, low=90 → lower shadow dominates → negative
        let mut rsb = RollingShadowBalance::new("rsb", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = rsb.update_bar(&bar("100", "100", "90", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(0), "lower shadow bias should be negative: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rsb_reset() {
        let mut rsb = RollingShadowBalance::new("rsb", 3).unwrap();
        for _ in 0..3 { rsb.update_bar(&bar("100", "110", "90", "100")).unwrap(); }
        assert!(rsb.is_ready());
        rsb.reset();
        assert!(!rsb.is_ready());
    }
}
