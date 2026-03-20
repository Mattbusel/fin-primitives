//! Awesome Oscillator indicator (Bill Williams).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

const FAST: usize = 5;
const SLOW: usize = 34;

/// Awesome Oscillator (AO) — momentum indicator measuring market driving force.
///
/// ```text
/// AO = SMA(median_price, 5) - SMA(median_price, 34)
/// median_price = (high + low) / 2
/// ```
///
/// Positive values indicate bullish momentum; negative values indicate bearish momentum.
/// Returns [`SignalValue::Unavailable`] until 34 bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AwesomeOscillator;
/// use fin_primitives::signals::Signal;
///
/// let ao = AwesomeOscillator::new("ao").unwrap();
/// assert_eq!(ao.period(), 34);
/// ```
pub struct AwesomeOscillator {
    name: String,
    history: VecDeque<Decimal>,
}

impl AwesomeOscillator {
    /// Constructs a new `AwesomeOscillator`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] only if the internal constants change; this
    /// constructor always succeeds for valid usage.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            history: VecDeque::with_capacity(SLOW),
        })
    }
}

impl Signal for AwesomeOscillator {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        SLOW
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= SLOW
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let median = (bar.high + bar.low) / Decimal::TWO;
        self.history.push_back(median);
        if self.history.len() > SLOW {
            self.history.pop_front();
        }
        if self.history.len() < SLOW {
            return Ok(SignalValue::Unavailable);
        }

        let slow_sum: Decimal = self.history.iter().copied().sum();
        let fast_sum: Decimal = self.history.iter().rev().take(FAST).sum();

        #[allow(clippy::cast_possible_truncation)]
        let slow_sma = slow_sum
            .checked_div(Decimal::from(SLOW as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        #[allow(clippy::cast_possible_truncation)]
        let fast_sma = fast_sum
            .checked_div(Decimal::from(FAST as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(fast_sma - slow_sma))
    }

    fn reset(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let mp = Price::new(((hp.value() + lp.value()) / Decimal::TWO).max(rust_decimal::Decimal::ONE)).unwrap();
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
    fn test_ao_unavailable_before_34_bars() {
        let mut ao = AwesomeOscillator::new("ao").unwrap();
        for _ in 0..33 {
            assert_eq!(ao.update_bar(&bar("101", "99")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!ao.is_ready());
    }

    #[test]
    fn test_ao_flat_market_zero() {
        let mut ao = AwesomeOscillator::new("ao").unwrap();
        for _ in 0..34 {
            ao.update_bar(&bar("101", "99")).unwrap();
        }
        // All medians equal: fast_sma == slow_sma => AO = 0
        assert_eq!(ao.is_ready(), true);
        if let SignalValue::Scalar(v) = ao.update_bar(&bar("101", "99")).unwrap() {
            assert_eq!(v, rust_decimal_macros::dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ao_reset() {
        let mut ao = AwesomeOscillator::new("ao").unwrap();
        for _ in 0..34 {
            ao.update_bar(&bar("101", "99")).unwrap();
        }
        assert!(ao.is_ready());
        ao.reset();
        assert!(!ao.is_ready());
    }
}
