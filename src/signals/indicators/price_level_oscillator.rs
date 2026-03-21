//! Price Level Oscillator indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Level Oscillator.
///
/// Computes the difference between two SMAs as a percentage of the longer-period SMA,
/// and adds a rolling Z-score context by tracking how unusual the current spread is.
///
/// This is the percentage price oscillator (PPO) — the difference between fast and slow
/// SMAs divided by the slow SMA, expressed as percentage:
///
/// Formula: `plo = (fast_sma - slow_sma) / slow_sma * 100`
///
/// - Positive: fast MA is above slow MA (uptrend).
/// - Negative: fast MA is below slow MA (downtrend).
/// - 0: MAs are equal.
///
/// Returns `SignalValue::Unavailable` until `slow_period` bars accumulated.
/// Returns `SignalValue::Scalar(0.0)` when slow SMA is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceLevelOscillator;
/// use fin_primitives::signals::Signal;
/// let plo = PriceLevelOscillator::new("plo_5_20", 5, 20).unwrap();
/// assert_eq!(plo.period(), 20);
/// ```
pub struct PriceLevelOscillator {
    name: String,
    fast_period: usize,
    slow_period: usize,
    closes: VecDeque<Decimal>,
}

impl PriceLevelOscillator {
    /// Constructs a new `PriceLevelOscillator`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is 0 or `fast_period >= slow_period`.
    pub fn new(
        name: impl Into<String>,
        fast_period: usize,
        slow_period: usize,
    ) -> Result<Self, FinError> {
        if fast_period == 0 {
            return Err(FinError::InvalidPeriod(fast_period));
        }
        if slow_period == 0 {
            return Err(FinError::InvalidPeriod(slow_period));
        }
        if fast_period >= slow_period {
            return Err(FinError::InvalidPeriod(fast_period));
        }
        Ok(Self {
            name: name.into(),
            fast_period,
            slow_period,
            closes: VecDeque::with_capacity(slow_period),
        })
    }
}

impl Signal for PriceLevelOscillator {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.slow_period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.slow_period {
            return Ok(SignalValue::Unavailable);
        }

        let vols: Vec<Decimal> = self.closes.iter().copied().collect();
        let slow_sum: Decimal = vols.iter().copied().sum();
        let fast_sum: Decimal = vols[vols.len() - self.fast_period..].iter().copied().sum();

        #[allow(clippy::cast_possible_truncation)]
        let slow_ma = slow_sum
            .checked_div(Decimal::from(self.slow_period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        #[allow(clippy::cast_possible_truncation)]
        let fast_ma = fast_sum
            .checked_div(Decimal::from(self.fast_period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if slow_ma.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let plo = (fast_ma - slow_ma)
            .checked_div(slow_ma)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(plo))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.slow_period
    }

    fn period(&self) -> usize {
        self.slow_period
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
        assert!(PriceLevelOscillator::new("plo", 0, 10).is_err());
        assert!(PriceLevelOscillator::new("plo", 10, 5).is_err());
        assert!(PriceLevelOscillator::new("plo", 5, 5).is_err());
    }

    #[test]
    fn test_unavailable_before_slow_period() {
        let mut plo = PriceLevelOscillator::new("plo", 2, 5).unwrap();
        assert_eq!(plo.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_equal_prices_zero() {
        let mut plo = PriceLevelOscillator::new("plo", 2, 4).unwrap();
        for _ in 0..4 {
            plo.update_bar(&bar("100")).unwrap();
        }
        let v = plo.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rising_gives_positive() {
        let mut plo = PriceLevelOscillator::new("plo", 2, 4).unwrap();
        for i in 1..=4u32 {
            plo.update_bar(&bar(&(100 + i * 5).to_string())).unwrap();
        }
        let v = plo.update_bar(&bar("140")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut plo = PriceLevelOscillator::new("plo", 2, 4).unwrap();
        for _ in 0..4 {
            plo.update_bar(&bar("100")).unwrap();
        }
        assert!(plo.is_ready());
        plo.reset();
        assert!(!plo.is_ready());
    }
}
