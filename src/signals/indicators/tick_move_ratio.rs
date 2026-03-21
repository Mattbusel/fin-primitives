//! Tick Move Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Tick Move Ratio.
///
/// Compares the net directional price move (close − open) to the total price path
/// (high − low) within each bar, then averages over a rolling window.
///
/// Per-bar formula: `tmr = (close − open) / (high − low)`
///
/// Rolling: `mean(tmr, period)`
///
/// - +1.0: consistently strong bullish moves with minimal wicks.
/// - −1.0: consistently strong bearish moves.
/// - ~0: choppy or wick-dominated action.
/// - Zero-range bars contribute 0.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TickMoveRatio;
/// use fin_primitives::signals::Signal;
/// let tmr = TickMoveRatio::new("tmr_14", 14).unwrap();
/// assert_eq!(tmr.period(), 14);
/// ```
pub struct TickMoveRatio {
    name: String,
    period: usize,
    ratios: VecDeque<Decimal>,
}

impl TickMoveRatio {
    /// Constructs a new `TickMoveRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, ratios: VecDeque::with_capacity(period) })
    }
}

impl Signal for TickMoveRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let tmr = if range.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - bar.open)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.ratios.push_back(tmr);
        if self.ratios.len() > self.period {
            self.ratios.pop_front();
        }
        if self.ratios.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.ratios.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.ratios.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.ratios.clear();
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
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(TickMoveRatio::new("tmr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut tmr = TickMoveRatio::new("tmr", 3).unwrap();
        assert_eq!(tmr.update_bar(&bar("10", "15", "8", "14")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_full_bullish_body() {
        let mut tmr = TickMoveRatio::new("tmr", 3).unwrap();
        for _ in 0..3 {
            // open=low, close=high → ratio=1
            tmr.update_bar(&bar("5", "10", "5", "10")).unwrap();
        }
        let v = tmr.update_bar(&bar("5", "10", "5", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_full_bearish_body() {
        let mut tmr = TickMoveRatio::new("tmr", 3).unwrap();
        for _ in 0..3 {
            // open=high, close=low → ratio=-1
            tmr.update_bar(&bar("10", "10", "5", "5")).unwrap();
        }
        let v = tmr.update_bar(&bar("10", "10", "5", "5")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_doji_zero() {
        let mut tmr = TickMoveRatio::new("tmr", 3).unwrap();
        for _ in 0..3 {
            // open == close → ratio=0
            tmr.update_bar(&bar("10", "12", "8", "10")).unwrap();
        }
        let v = tmr.update_bar(&bar("10", "12", "8", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut tmr = TickMoveRatio::new("tmr", 2).unwrap();
        tmr.update_bar(&bar("5", "10", "5", "10")).unwrap();
        tmr.update_bar(&bar("5", "10", "5", "10")).unwrap();
        assert!(tmr.is_ready());
        tmr.reset();
        assert!(!tmr.is_ready());
    }
}
