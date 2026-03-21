//! Dual Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Dual Momentum.
///
/// Combines absolute momentum (trend direction) with relative momentum (acceleration)
/// into a single composite signal, inspired by Gary Antonacci's Dual Momentum framework.
///
/// Components:
/// - `abs_mom = (close_t / close_{t - long_period} - 1)` — absolute (trend) momentum.
/// - `rel_mom = (close_t / close_{t - short_period} - 1)` — relative (recent) momentum.
/// - `dual = (abs_mom + rel_mom) / 2` — simple average of both.
///
/// Output is a decimal return value (e.g., 0.15 = 15% combined momentum).
///
/// Returns `SignalValue::Unavailable` until `long_period + 1` closes accumulated.
/// Returns `SignalValue::Scalar(0.0)` when any base price is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DualMomentum;
/// use fin_primitives::signals::Signal;
/// let dm = DualMomentum::new("dm_12_1", 12, 1).unwrap();
/// assert_eq!(dm.period(), 12);
/// ```
pub struct DualMomentum {
    name: String,
    long_period: usize,
    short_period: usize,
    closes: VecDeque<Decimal>,
}

impl DualMomentum {
    /// Constructs a new `DualMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is 0 or `short_period >= long_period`.
    pub fn new(
        name: impl Into<String>,
        long_period: usize,
        short_period: usize,
    ) -> Result<Self, FinError> {
        if long_period == 0 {
            return Err(FinError::InvalidPeriod(long_period));
        }
        if short_period == 0 {
            return Err(FinError::InvalidPeriod(short_period));
        }
        if short_period >= long_period {
            return Err(FinError::InvalidPeriod(short_period));
        }
        Ok(Self {
            name: name.into(),
            long_period,
            short_period,
            closes: VecDeque::with_capacity(long_period + 1),
        })
    }
}

impl Signal for DualMomentum {
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
        let long_base = *self.closes.front().unwrap();
        let short_base_idx = self.long_period - self.short_period;
        let short_base = self.closes[short_base_idx];

        if long_base.is_zero() || short_base.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let abs_mom = (current - long_base)
            .checked_div(long_base)
            .ok_or(FinError::ArithmeticOverflow)?;
        let rel_mom = (current - short_base)
            .checked_div(short_base)
            .ok_or(FinError::ArithmeticOverflow)?;

        let dual = (abs_mom + rel_mom)
            .checked_div(Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(dual))
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
        assert!(DualMomentum::new("dm", 0, 1).is_err());
        assert!(DualMomentum::new("dm", 5, 0).is_err());
        assert!(DualMomentum::new("dm", 3, 5).is_err());
        assert!(DualMomentum::new("dm", 5, 5).is_err());
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut dm = DualMomentum::new("dm", 5, 2).unwrap();
        assert_eq!(dm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_flat_price_zero_momentum() {
        let mut dm = DualMomentum::new("dm", 4, 2).unwrap();
        for _ in 0..5 {
            dm.update_bar(&bar("100")).unwrap();
        }
        let v = dm.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rising_price_positive_momentum() {
        let mut dm = DualMomentum::new("dm", 4, 2).unwrap();
        for i in 1..=5u32 {
            dm.update_bar(&bar(&(100 + i * 2).to_string())).unwrap();
        }
        let v = dm.update_bar(&bar("115")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut dm = DualMomentum::new("dm", 3, 1).unwrap();
        for _ in 0..4 {
            dm.update_bar(&bar("100")).unwrap();
        }
        assert!(dm.is_ready());
        dm.reset();
        assert!(!dm.is_ready());
    }
}
