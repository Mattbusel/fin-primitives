//! Rate of Change Percentage indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rate of Change Percentage (ROCP).
///
/// Measures the percentage price change from `period` bars ago to the current bar.
/// Unlike the standard ROC (which is the ratio minus 1), ROCP returns the raw
/// percentage change: `(close_t - close_{t-period}) / close_{t-period} * 100`.
///
/// This is a standard ROC expressed as a percentage rather than a ratio.
///
/// Returns `SignalValue::Unavailable` until `period + 1` closes accumulated.
/// Returns `SignalValue::Scalar(0.0)` when the base price is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RateOfChangePct;
/// use fin_primitives::signals::Signal;
/// let rocp = RateOfChangePct::new("rocp_14", 14).unwrap();
/// assert_eq!(rocp.period(), 14);
/// ```
pub struct RateOfChangePct {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl RateOfChangePct {
    /// Constructs a new `RateOfChangePct`.
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
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for RateOfChangePct {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let current = *self.closes.back().unwrap();
        let base = *self.closes.front().unwrap();

        if base.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let rocp = (current - base)
            .checked_div(base)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(rocp))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period + 1
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_period_zero_fails() {
        assert!(matches!(RateOfChangePct::new("rocp", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut rocp = RateOfChangePct::new("rocp", 3).unwrap();
        assert_eq!(rocp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ten_pct_gain() {
        // 100 → 110 over period=1 → 10%
        let mut rocp = RateOfChangePct::new("rocp", 1).unwrap();
        rocp.update_bar(&bar("100")).unwrap();
        let v = rocp.update_bar(&bar("110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_flat_is_zero() {
        let mut rocp = RateOfChangePct::new("rocp", 3).unwrap();
        for _ in 0..4 {
            rocp.update_bar(&bar("100")).unwrap();
        }
        let v = rocp.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_loss_negative() {
        // 100 → 90 → -10%
        let mut rocp = RateOfChangePct::new("rocp", 1).unwrap();
        rocp.update_bar(&bar("100")).unwrap();
        let v = rocp.update_bar(&bar("90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_reset() {
        let mut rocp = RateOfChangePct::new("rocp", 2).unwrap();
        for _ in 0..3 {
            rocp.update_bar(&bar("100")).unwrap();
        }
        assert!(rocp.is_ready());
        rocp.reset();
        assert!(!rocp.is_ready());
    }
}
