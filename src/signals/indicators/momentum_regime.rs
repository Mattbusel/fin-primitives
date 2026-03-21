//! Momentum Regime indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Momentum Regime.
///
/// Classifies the current momentum regime by comparing the short-term Rate of Change
/// (ROC) to the long-term ROC. The regime is expressed as the ratio of short ROC to
/// long ROC, indicating whether near-term momentum is accelerating or decelerating
/// relative to the broader trend.
///
/// Formula:
/// - `short_roc = (close_t / close_{t - short_period} - 1) * 100`
/// - `long_roc = (close_t / close_{t - long_period} - 1) * 100`
/// - `regime = short_roc / long_roc` (when long_roc != 0)
///
/// Interpretation:
/// - > 1.0: momentum accelerating (short term stronger than long term).
/// - 0 to 1.0: momentum decelerating but same direction.
/// - < 0: short and long term momentum diverging (potential reversal).
/// - Returns `0.0` when long_roc is zero.
///
/// Returns `SignalValue::Unavailable` until `long_period + 1` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MomentumRegime;
/// use fin_primitives::signals::Signal;
/// let mr = MomentumRegime::new("mr_5_20", 5, 20).unwrap();
/// assert_eq!(mr.period(), 20);
/// ```
pub struct MomentumRegime {
    name: String,
    short_period: usize,
    long_period: usize,
    closes: VecDeque<Decimal>,
}

impl MomentumRegime {
    /// Constructs a new `MomentumRegime`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `short_period == 0`, `long_period == 0`,
    /// or `short_period >= long_period`.
    pub fn new(
        name: impl Into<String>,
        short_period: usize,
        long_period: usize,
    ) -> Result<Self, FinError> {
        if short_period == 0 {
            return Err(FinError::InvalidPeriod(short_period));
        }
        if long_period == 0 {
            return Err(FinError::InvalidPeriod(long_period));
        }
        if short_period >= long_period {
            return Err(FinError::InvalidPeriod(short_period));
        }
        Ok(Self {
            name: name.into(),
            short_period,
            long_period,
            closes: VecDeque::with_capacity(long_period + 1),
        })
    }
}

impl Signal for MomentumRegime {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.long_period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.long_period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let current = *self.closes.back().unwrap();
        let long_base = *self.closes.front().unwrap(); // long_period bars ago
        let short_base_idx = self.long_period - self.short_period;
        let short_base = self.closes[short_base_idx];

        if long_base.is_zero() || short_base.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let long_roc = (current - long_base)
            .checked_div(long_base)
            .ok_or(FinError::ArithmeticOverflow)?;

        if long_roc.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let short_roc = (current - short_base)
            .checked_div(short_base)
            .ok_or(FinError::ArithmeticOverflow)?;

        let regime = short_roc.checked_div(long_roc).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(regime))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.long_period + 1
    }

    fn period(&self) -> usize {
        self.long_period
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
    fn test_invalid_params() {
        assert!(MomentumRegime::new("mr", 0, 20).is_err());
        assert!(MomentumRegime::new("mr", 20, 5).is_err());
        assert!(MomentumRegime::new("mr", 5, 5).is_err());
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut mr = MomentumRegime::new("mr", 2, 5).unwrap();
        assert_eq!(mr.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_flat_returns_zero() {
        let mut mr = MomentumRegime::new("mr", 2, 4).unwrap();
        for _ in 0..5 {
            mr.update_bar(&bar("100")).unwrap();
        }
        let v = mr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut mr = MomentumRegime::new("mr", 2, 4).unwrap();
        for _ in 0..5 {
            mr.update_bar(&bar("100")).unwrap();
        }
        assert!(mr.is_ready());
        mr.reset();
        assert!(!mr.is_ready());
    }
}
