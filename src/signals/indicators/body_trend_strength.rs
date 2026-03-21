//! Body Trend Strength indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body Trend Strength.
///
/// Measures the net directional bias of candlestick bodies over a rolling window.
/// Each bar contributes its signed body: `+body` for bullish, `−body` for bearish,
/// normalized by the total absolute body sum for the window.
///
/// Formula: `bts = Σ signed_body / Σ |body|`
///
/// - Range: [−1, +1].
/// - +1: all bars are fully bullish.
/// - −1: all bars are fully bearish.
/// - 0: balanced or all doji bars.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
/// Returns `SignalValue::Scalar(0.0)` when total absolute body is zero (all dojis).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyTrendStrength;
/// use fin_primitives::signals::Signal;
/// let bts = BodyTrendStrength::new("bts_14", 14).unwrap();
/// assert_eq!(bts.period(), 14);
/// ```
pub struct BodyTrendStrength {
    name: String,
    period: usize,
    bodies: VecDeque<Decimal>,
}

impl BodyTrendStrength {
    /// Constructs a new `BodyTrendStrength` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, bodies: VecDeque::with_capacity(period) })
    }
}

impl Signal for BodyTrendStrength {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Signed body: positive if bullish, negative if bearish
        let signed_body = bar.close - bar.open;
        self.bodies.push_back(signed_body);
        if self.bodies.len() > self.period {
            self.bodies.pop_front();
        }
        if self.bodies.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let net: Decimal = self.bodies.iter().copied().sum();
        let total_abs: Decimal = self.bodies.iter().map(|b| b.abs()).sum();

        if total_abs.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let bts = net.checked_div(total_abs).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(bts))
    }

    fn is_ready(&self) -> bool {
        self.bodies.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.bodies.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let (lo, hi) = if op <= cl { (op, cl) } else { (cl, op) };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(BodyTrendStrength::new("bts", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut bts = BodyTrendStrength::new("bts", 3).unwrap();
        assert_eq!(bts.update_bar(&bar("10", "12")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_all_bullish_returns_one() {
        let mut bts = BodyTrendStrength::new("bts", 3).unwrap();
        for _ in 0..3 {
            bts.update_bar(&bar("10", "12")).unwrap();
        }
        let v = bts.update_bar(&bar("10", "12")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_all_bearish_returns_neg_one() {
        let mut bts = BodyTrendStrength::new("bts", 3).unwrap();
        for _ in 0..3 {
            bts.update_bar(&bar("12", "10")).unwrap();
        }
        let v = bts.update_bar(&bar("12", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_mixed_near_zero() {
        let mut bts = BodyTrendStrength::new("bts", 2).unwrap();
        bts.update_bar(&bar("10", "12")).unwrap(); // +2
        let v = bts.update_bar(&bar("12", "10")).unwrap(); // -2 → net=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut bts = BodyTrendStrength::new("bts", 2).unwrap();
        bts.update_bar(&bar("10", "12")).unwrap();
        bts.update_bar(&bar("10", "12")).unwrap();
        assert!(bts.is_ready());
        bts.reset();
        assert!(!bts.is_ready());
    }
}
