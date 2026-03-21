//! Price Action Index indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Action Index (PAI).
///
/// A composite score measuring bar quality and direction from three components:
/// - **Body ratio**: body size / range (commitment strength).
/// - **Close position**: where close lands in the range ((close-low)/(high-low)).
/// - **Direction**: +1 bullish body, −1 bearish body.
///
/// Per-bar: `pai = direction * body_ratio * close_position`
///
/// Rolling: `mean(pai, period)` — ranges from [−1, +1].
///
/// - Positive → net bullish price action with strong closes near highs.
/// - Negative → net bearish price action with weak closes near lows.
/// - Zero-range bars contribute 0.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceActionIndex;
/// use fin_primitives::signals::Signal;
/// let pai = PriceActionIndex::new("pai_14", 14).unwrap();
/// assert_eq!(pai.period(), 14);
/// ```
pub struct PriceActionIndex {
    name: String,
    period: usize,
    scores: VecDeque<Decimal>,
}

impl PriceActionIndex {
    /// Constructs a new `PriceActionIndex`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, scores: VecDeque::with_capacity(period) })
    }
}

impl Signal for PriceActionIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let score = if range.is_zero() {
            Decimal::ZERO
        } else {
            let body_ratio = (bar.close - bar.open)
                .abs()
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?;
            let close_pos = (bar.close - bar.low)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?;
            let direction = if bar.close >= bar.open {
                Decimal::ONE
            } else {
                Decimal::NEGATIVE_ONE
            };
            direction
                .checked_mul(body_ratio)
                .ok_or(FinError::ArithmeticOverflow)?
                .checked_mul(close_pos)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.scores.push_back(score);
        if self.scores.len() > self.period {
            self.scores.pop_front();
        }
        if self.scores.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.scores.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.scores.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.scores.clear();
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
        assert!(matches!(PriceActionIndex::new("pai", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut pai = PriceActionIndex::new("pai", 3).unwrap();
        assert_eq!(pai.update_bar(&bar("10", "15", "8", "14")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bullish_close_at_high() {
        let mut pai = PriceActionIndex::new("pai", 3).unwrap();
        for _ in 0..3 {
            // open=5, high=10, low=5, close=10 → body_ratio=1, close_pos=1, dir=+1 → score=1
            pai.update_bar(&bar("5", "10", "5", "10")).unwrap();
        }
        let v = pai.update_bar(&bar("5", "10", "5", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bearish_close_at_low() {
        let mut pai = PriceActionIndex::new("pai", 3).unwrap();
        for _ in 0..3 {
            // open=10, high=10, low=5, close=5 → body_ratio=1, close_pos=0, dir=-1 → score=0
            pai.update_bar(&bar("10", "10", "5", "5")).unwrap();
        }
        let v = pai.update_bar(&bar("10", "10", "5", "5")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut pai = PriceActionIndex::new("pai", 2).unwrap();
        pai.update_bar(&bar("5", "10", "5", "10")).unwrap();
        pai.update_bar(&bar("5", "10", "5", "10")).unwrap();
        assert!(pai.is_ready());
        pai.reset();
        assert!(!pai.is_ready());
    }
}
