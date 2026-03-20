//! Price Above Rolling High indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Above Rolling High — outputs `+1` when the current close exceeds the
/// highest close in the previous `period` bars (excluding the current bar),
/// and `0` otherwise.
///
/// This is a classic Donchian-style breakout signal.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceAboveRollingHigh;
/// use fin_primitives::signals::Signal;
///
/// let parh = PriceAboveRollingHigh::new("parh", 20).unwrap();
/// assert_eq!(parh.period(), 20);
/// ```
pub struct PriceAboveRollingHigh {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PriceAboveRollingHigh {
    /// Constructs a new `PriceAboveRollingHigh`.
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
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for PriceAboveRollingHigh {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Compare against historical closes BEFORE adding current
        let result = if self.closes.len() >= self.period {
            let max_close = self.closes.iter().copied().fold(self.closes[0], |acc, v| acc.max(v));
            if bar.close > max_close {
                SignalValue::Scalar(Decimal::ONE)
            } else {
                SignalValue::Scalar(Decimal::ZERO)
            }
        } else {
            SignalValue::Unavailable
        };

        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }

        Ok(result)
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
    fn test_parh_invalid_period() {
        assert!(PriceAboveRollingHigh::new("parh", 0).is_err());
    }

    #[test]
    fn test_parh_unavailable_before_warm_up() {
        let mut parh = PriceAboveRollingHigh::new("parh", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(parh.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_parh_breakout_gives_one() {
        let mut parh = PriceAboveRollingHigh::new("parh", 3).unwrap();
        for _ in 0..3 { parh.update_bar(&bar("100")).unwrap(); }
        // close=150 > max(100,100,100) → +1
        let result = parh.update_bar(&bar("150")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_parh_no_breakout_gives_zero() {
        let mut parh = PriceAboveRollingHigh::new("parh", 3).unwrap();
        for _ in 0..3 { parh.update_bar(&bar("100")).unwrap(); }
        // close=80 < max → 0
        let result = parh.update_bar(&bar("80")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_parh_reset() {
        let mut parh = PriceAboveRollingHigh::new("parh", 3).unwrap();
        for _ in 0..3 { parh.update_bar(&bar("100")).unwrap(); }
        assert!(parh.is_ready());
        parh.reset();
        assert!(!parh.is_ready());
    }
}
