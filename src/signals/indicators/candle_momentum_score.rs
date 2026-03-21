//! Candle Momentum Score indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Candle Momentum Score.
///
/// Combines three factors per bar into a single momentum score:
/// 1. **Direction**: +1 bullish, −1 bearish, 0 doji.
/// 2. **Body size**: body / range (commitment ratio ∈ [0,1]).
/// 3. **Close position within range**: (close − low) / range ∈ [0,1], scaled to [−1,+1].
///
/// Per-bar score: `(direction * body_ratio + (2 * close_pos - 1)) / 2`
///
/// Rolling: `mean(score, period)`
///
/// Range: [−1, +1].
/// - +1: perfect bullish candle (full body, close at high).
/// - −1: perfect bearish candle (full body, close at low).
/// - 0: neutral/choppy.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
/// Zero-range bars contribute 0.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CandleMomentumScore;
/// use fin_primitives::signals::Signal;
/// let cms = CandleMomentumScore::new("cms_14", 14).unwrap();
/// assert_eq!(cms.period(), 14);
/// ```
pub struct CandleMomentumScore {
    name: String,
    period: usize,
    scores: VecDeque<Decimal>,
}

impl CandleMomentumScore {
    /// Constructs a new `CandleMomentumScore`.
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

impl Signal for CandleMomentumScore {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let score = if range.is_zero() {
            Decimal::ZERO
        } else {
            let body = (bar.close - bar.open).abs();
            let body_ratio = body.checked_div(range).ok_or(FinError::ArithmeticOverflow)?;
            let direction = if bar.close > bar.open {
                Decimal::ONE
            } else if bar.close < bar.open {
                Decimal::NEGATIVE_ONE
            } else {
                Decimal::ZERO
            };
            let close_pos = (bar.close - bar.low).checked_div(range).ok_or(FinError::ArithmeticOverflow)?;
            // close_pos_scaled ∈ [-1, 1]
            let close_pos_scaled = close_pos
                .checked_mul(Decimal::TWO)
                .ok_or(FinError::ArithmeticOverflow)?
                - Decimal::ONE;

            let component1 = direction.checked_mul(body_ratio).ok_or(FinError::ArithmeticOverflow)?;
            let sum = component1 + close_pos_scaled;
            sum.checked_div(Decimal::TWO).ok_or(FinError::ArithmeticOverflow)?
        };

        self.scores.push_back(score);
        if self.scores.len() > self.period {
            self.scores.pop_front();
        }
        if self.scores.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let total: Decimal = self.scores.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = total
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
        assert!(matches!(CandleMomentumScore::new("cms", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut cms = CandleMomentumScore::new("cms", 3).unwrap();
        assert_eq!(cms.update_bar(&bar("10", "15", "8", "12")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_perfect_bullish_gives_one() {
        let mut cms = CandleMomentumScore::new("cms", 3).unwrap();
        for _ in 0..3 {
            // open=low, close=high → body_ratio=1, close_pos=1, dir=+1
            // score = (1*1 + (2*1-1))/2 = (1+1)/2 = 1
            cms.update_bar(&bar("5", "15", "5", "15")).unwrap();
        }
        let v = cms.update_bar(&bar("5", "15", "5", "15")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_reset() {
        let mut cms = CandleMomentumScore::new("cms", 2).unwrap();
        cms.update_bar(&bar("5", "15", "5", "15")).unwrap();
        cms.update_bar(&bar("5", "15", "5", "15")).unwrap();
        assert!(cms.is_ready());
        cms.reset();
        assert!(!cms.is_ready());
    }
}
