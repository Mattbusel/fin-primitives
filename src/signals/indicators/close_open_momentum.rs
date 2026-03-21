//! Close-Open Momentum indicator.
//!
//! Computes the rolling sum of signed intrabar moves (close - open), tracking
//! the cumulative directional commitment of recent bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close-Open Momentum: rolling sum of `(close - open)` over `period` bars.
///
/// Unlike momentum based on close-to-close, this indicator measures only the
/// intrabar directional move (the body direction and magnitude). It ignores
/// overnight gaps, making it useful for isolating session-level directional
/// pressure from gap effects.
///
/// - **Positive**: more bullish body commitment across the window.
/// - **Negative**: more bearish body commitment.
/// - **Near zero**: bodies are offsetting each other (chop).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseOpenMomentum;
/// use fin_primitives::signals::Signal;
///
/// let com = CloseOpenMomentum::new("com", 5).unwrap();
/// assert_eq!(com.period(), 5);
/// assert!(!com.is_ready());
/// ```
pub struct CloseOpenMomentum {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseOpenMomentum {
    /// Constructs a new `CloseOpenMomentum`.
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

impl Signal for CloseOpenMomentum {
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
        let net = bar.net_move();

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
        let high = if c >= o { c } else { o };
        let low = if c <= o { c } else { o };
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
    fn test_com_invalid_period() {
        assert!(CloseOpenMomentum::new("com", 0).is_err());
    }

    #[test]
    fn test_com_unavailable_during_warmup() {
        let mut com = CloseOpenMomentum::new("com", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(com.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_com_all_bullish_positive() {
        let mut com = CloseOpenMomentum::new("com", 3).unwrap();
        for _ in 0..3 {
            com.update_bar(&bar("100", "110")).unwrap();
        }
        let v = com.update_bar(&bar("100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(30)));
    }

    #[test]
    fn test_com_all_bearish_negative() {
        let mut com = CloseOpenMomentum::new("com", 3).unwrap();
        for _ in 0..3 {
            com.update_bar(&bar("110", "100")).unwrap();
        }
        let v = com.update_bar(&bar("110", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-30)));
    }

    #[test]
    fn test_com_doji_zero() {
        let mut com = CloseOpenMomentum::new("com", 3).unwrap();
        for _ in 0..3 {
            com.update_bar(&bar("100", "100")).unwrap();
        }
        let v = com.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_com_rolling_window() {
        let mut com = CloseOpenMomentum::new("com", 2).unwrap();
        com.update_bar(&bar("100", "110")).unwrap(); // net = 10, window=[10]
        com.update_bar(&bar("100", "110")).unwrap(); // net = 10, window=[10,10], sum=20
        // Now add bearish bar: net=-5. Window slides: [10,-5], sum=5
        let v = com.update_bar(&bar("110", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(5)));
    }

    #[test]
    fn test_com_reset() {
        let mut com = CloseOpenMomentum::new("com", 3).unwrap();
        for _ in 0..3 {
            com.update_bar(&bar("100", "105")).unwrap();
        }
        assert!(com.is_ready());
        com.reset();
        assert!(!com.is_ready());
    }
}
