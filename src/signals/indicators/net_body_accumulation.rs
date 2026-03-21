//! Net Body Accumulation indicator.
//!
//! Tracks the rolling sum of `(close - open)` over a period window, measuring
//! the net directional progress of price across all bars in the window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling sum of `(close − open)` over `period` bars.
///
/// Positive values indicate net upward movement across the window (more bullish
/// bar bodies than bearish). Negative values indicate net downward movement.
/// The magnitude reflects cumulative directional progress in price units.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NetBodyAccumulation;
/// use fin_primitives::signals::Signal;
///
/// let nba = NetBodyAccumulation::new("nba", 10).unwrap();
/// assert_eq!(nba.period(), 10);
/// assert!(!nba.is_ready());
/// ```
pub struct NetBodyAccumulation {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl NetBodyAccumulation {
    /// Constructs a new `NetBodyAccumulation`.
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
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl crate::signals::Signal for NetBodyAccumulation {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let net = bar.close - bar.open;

        self.sum += net;
        self.window.push_back(net);

        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        Ok(SignalValue::Scalar(self.sum))
    }

    fn reset(&mut self) {
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

    fn bar(open: &str, close: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let (high, low) = if c >= o { (c, o) } else { (o, c) };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o, high, low, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_nba_invalid_period() {
        assert!(NetBodyAccumulation::new("nba", 0).is_err());
    }

    #[test]
    fn test_nba_unavailable_during_warmup() {
        let mut nba = NetBodyAccumulation::new("nba", 3).unwrap();
        assert_eq!(nba.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(nba.update_bar(&bar("105", "110")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_nba_all_bullish_positive() {
        let mut nba = NetBodyAccumulation::new("nba", 3).unwrap();
        nba.update_bar(&bar("100", "105")).unwrap(); // +5
        nba.update_bar(&bar("105", "110")).unwrap(); // +5
        let v = nba.update_bar(&bar("110", "115")).unwrap(); // +5 → sum=15
        assert_eq!(v, SignalValue::Scalar(dec!(15)));
    }

    #[test]
    fn test_nba_mixed_sums_correctly() {
        let mut nba = NetBodyAccumulation::new("nba", 3).unwrap();
        nba.update_bar(&bar("100", "110")).unwrap(); // +10
        nba.update_bar(&bar("110", "105")).unwrap(); // -5
        let v = nba.update_bar(&bar("105", "108")).unwrap(); // +3 → sum=8
        assert_eq!(v, SignalValue::Scalar(dec!(8)));
    }

    #[test]
    fn test_nba_sliding_window() {
        let mut nba = NetBodyAccumulation::new("nba", 2).unwrap();
        nba.update_bar(&bar("100", "110")).unwrap(); // +10
        nba.update_bar(&bar("110", "105")).unwrap(); // -5 → sum=5
        // Next bar evicts the +10
        let v = nba.update_bar(&bar("105", "103")).unwrap(); // -2 → sum=-5-2=-7
        assert_eq!(v, SignalValue::Scalar(dec!(-7)));
    }

    #[test]
    fn test_nba_reset() {
        let mut nba = NetBodyAccumulation::new("nba", 3).unwrap();
        for _ in 0..3 {
            nba.update_bar(&bar("100", "105")).unwrap();
        }
        assert!(nba.is_ready());
        nba.reset();
        assert!(!nba.is_ready());
    }
}
