//! Golden Cross / Death Cross Signal indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Golden Cross / Death Cross Signal.
///
/// Tracks the relationship between a fast and slow Simple Moving Average (SMA),
/// signaling classic trend changes:
///
/// Output:
/// - `+1.0`: fast SMA > slow SMA (golden cross territory — bullish).
/// - `−1.0`: fast SMA < slow SMA (death cross territory — bearish).
/// - `0.0`: fast SMA == slow SMA.
///
/// Returns `SignalValue::Unavailable` until `slow_period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GoldenCrossSignal;
/// use fin_primitives::signals::Signal;
/// let gc = GoldenCrossSignal::new("gc_50_200", 50, 200).unwrap();
/// assert_eq!(gc.period(), 200);
/// ```
pub struct GoldenCrossSignal {
    name: String,
    fast_period: usize,
    slow_period: usize,
    closes: VecDeque<Decimal>,
}

impl GoldenCrossSignal {
    /// Constructs a new `GoldenCrossSignal`.
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

    fn sma(window: &[Decimal]) -> Result<Decimal, FinError> {
        let sum: Decimal = window.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        sum.checked_div(Decimal::from(window.len() as u32))
            .ok_or(FinError::ArithmeticOverflow)
    }
}

impl Signal for GoldenCrossSignal {
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

        let slow_slice: Vec<Decimal> = self.closes.iter().copied().collect();
        let fast_slice = &slow_slice[slow_slice.len() - self.fast_period..];

        let fast_sma = Self::sma(fast_slice)?;
        let slow_sma = Self::sma(&slow_slice)?;

        let signal = if fast_sma > slow_sma {
            Decimal::ONE
        } else if fast_sma < slow_sma {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::ZERO
        };
        Ok(SignalValue::Scalar(signal))
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
        assert!(GoldenCrossSignal::new("gc", 0, 10).is_err());
        assert!(GoldenCrossSignal::new("gc", 10, 5).is_err());
        assert!(GoldenCrossSignal::new("gc", 10, 10).is_err());
    }

    #[test]
    fn test_unavailable_before_slow_period() {
        let mut gc = GoldenCrossSignal::new("gc", 2, 5).unwrap();
        assert_eq!(gc.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_golden_cross_rising_trend() {
        let mut gc = GoldenCrossSignal::new("gc", 2, 5).unwrap();
        // Rising prices: fast SMA > slow SMA
        for i in 1..=5u32 {
            gc.update_bar(&bar(&(100 + i * 5).to_string())).unwrap();
        }
        let v = gc.update_bar(&bar("140")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_death_cross_falling_trend() {
        let mut gc = GoldenCrossSignal::new("gc", 2, 5).unwrap();
        // Falling prices: fast SMA < slow SMA
        for i in 0..5u32 {
            gc.update_bar(&bar(&(140 - i * 5).to_string())).unwrap();
        }
        let v = gc.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_reset() {
        let mut gc = GoldenCrossSignal::new("gc", 2, 4).unwrap();
        for i in 1..=4u32 {
            gc.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(gc.is_ready());
        gc.reset();
        assert!(!gc.is_ready());
    }
}
