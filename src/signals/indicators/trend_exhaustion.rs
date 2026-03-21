//! Trend Exhaustion indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Exhaustion.
///
/// Identifies potential trend exhaustion by comparing the most recent bar's price
/// move to the average move per bar over the lookback period. Very large moves
/// relative to the average can signal exhaustion (parabolic move about to reverse).
///
/// Formula:
/// - `avg_move = |close_{t} - close_{t - period}| / period`  (average abs move per bar)
/// - `last_move = |close_t - close_{t-1}|` (most recent bar's absolute move)
/// - `exhaustion = last_move / avg_move` (ratio)
///
/// High values (> 2.0–3.0) may indicate exhaustion or acceleration.
/// Returns 0.0 when average move is zero.
///
/// Returns `SignalValue::Unavailable` until `period + 1` closes accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendExhaustion;
/// use fin_primitives::signals::Signal;
/// let te = TrendExhaustion::new("te_20", 20).unwrap();
/// assert_eq!(te.period(), 20);
/// ```
pub struct TrendExhaustion {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl TrendExhaustion {
    /// Constructs a new `TrendExhaustion`.
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
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for TrendExhaustion {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let current = *self.closes.back().unwrap();
        let prev = self.closes[self.closes.len() - 2];
        let period_start = *self.closes.front().unwrap();

        let last_move = (current - prev).abs();
        let total_move = (current - period_start).abs();
        #[allow(clippy::cast_possible_truncation)]
        let avg_move = total_move
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if avg_move.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let exhaustion = last_move.checked_div(avg_move).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(exhaustion))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period + 1
    }

    fn period(&self) -> usize {
        self.period
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
    fn test_period_zero_fails() {
        assert!(matches!(TrendExhaustion::new("te", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut te = TrendExhaustion::new("te", 3).unwrap();
        assert_eq!(te.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_steady_trend_gives_one() {
        // Constant move of 1 per bar → avg_move=1, last_move=1 → ratio=1
        let mut te = TrendExhaustion::new("te", 3).unwrap();
        te.update_bar(&bar("100")).unwrap();
        te.update_bar(&bar("101")).unwrap();
        te.update_bar(&bar("102")).unwrap();
        let v = te.update_bar(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_flat_returns_zero() {
        let mut te = TrendExhaustion::new("te", 3).unwrap();
        for _ in 0..4 {
            te.update_bar(&bar("100")).unwrap();
        }
        let v = te.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut te = TrendExhaustion::new("te", 2).unwrap();
        te.update_bar(&bar("100")).unwrap();
        te.update_bar(&bar("101")).unwrap();
        te.update_bar(&bar("102")).unwrap();
        assert!(te.is_ready());
        te.reset();
        assert!(!te.is_ready());
    }
}
