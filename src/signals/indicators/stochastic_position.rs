//! Stochastic Position indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Stochastic Position.
///
/// Similar to the %K line of the classic stochastic oscillator, but without smoothing.
/// Measures where the current close sits within the period's high-low range, expressed
/// as a percentage [0, 100].
///
/// Formula: `sp = (close - lowest_low) / (highest_high - lowest_low) * 100`
///
/// - 100: close at the period high.
/// - 0: close at the period low.
/// - 50: close at the midpoint.
/// - Returns 50 when range is zero (all prices equal).
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::StochasticPosition;
/// use fin_primitives::signals::Signal;
/// let sp = StochasticPosition::new("sp_14", 14).unwrap();
/// assert_eq!(sp.period(), 14);
/// ```
pub struct StochasticPosition {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    last_close: Decimal,
}

impl StochasticPosition {
    /// Constructs a new `StochasticPosition`.
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
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
            last_close: Decimal::ZERO,
        })
    }
}

impl Signal for StochasticPosition {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        self.last_close = bar.close;

        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let highest_high = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let lowest_low = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let range = highest_high - lowest_low;

        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::from(50u32)));
        }

        let sp = (self.last_close - lowest_low)
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(sp))
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.last_close = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(StochasticPosition::new("sp", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut sp = StochasticPosition::new("sp", 3).unwrap();
        assert_eq!(sp.update_bar(&bar("12", "10", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_close_at_period_high_gives_100() {
        let mut sp = StochasticPosition::new("sp", 3).unwrap();
        sp.update_bar(&bar("12", "8", "10")).unwrap();
        sp.update_bar(&bar("12", "8", "10")).unwrap();
        // Close at period high=14 → 100
        let v = sp.update_bar(&bar("14", "8", "14")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_close_at_period_low_gives_zero() {
        let mut sp = StochasticPosition::new("sp", 3).unwrap();
        sp.update_bar(&bar("14", "8", "11")).unwrap();
        sp.update_bar(&bar("14", "8", "11")).unwrap();
        // Close at period low=8 → 0
        let v = sp.update_bar(&bar("14", "8", "8")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut sp = StochasticPosition::new("sp", 2).unwrap();
        sp.update_bar(&bar("12", "10", "11")).unwrap();
        sp.update_bar(&bar("12", "10", "11")).unwrap();
        assert!(sp.is_ready());
        sp.reset();
        assert!(!sp.is_ready());
    }
}
