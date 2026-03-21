//! Closing Strength indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Closing Strength.
///
/// Measures how strongly price closes relative to its intrabar range over a rolling window.
/// This is a rolled version of the Williams %R concept but measured from 0 to 100:
///
/// Per-bar: `cs = (close - low) / (high - low) * 100`
///
/// Rolling: `mean(cs, period)`
///
/// - 100: consistently closes at the high of the bar (maximum buying pressure).
/// - 0: consistently closes at the low (maximum selling pressure).
/// - 50: neutral.
/// - Zero-range bars contribute 50 (neutral, neither bullish nor bearish).
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ClosingStrength;
/// use fin_primitives::signals::Signal;
/// let cs = ClosingStrength::new("cs_14", 14).unwrap();
/// assert_eq!(cs.period(), 14);
/// ```
pub struct ClosingStrength {
    name: String,
    period: usize,
    values: VecDeque<Decimal>,
}

impl ClosingStrength {
    /// Constructs a new `ClosingStrength`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, values: VecDeque::with_capacity(period) })
    }
}

impl Signal for ClosingStrength {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let cs = if range.is_zero() {
            Decimal::from(50u32) // neutral for doji
        } else {
            (bar.close - bar.low)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
                .checked_mul(Decimal::from(100u32))
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.values.push_back(cs);
        if self.values.len() > self.period {
            self.values.pop_front();
        }
        if self.values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.values.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.values.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.values.clear();
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
        assert!(matches!(ClosingStrength::new("cs", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut cs = ClosingStrength::new("cs", 3).unwrap();
        assert_eq!(cs.update_bar(&bar("12", "10", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_close_at_high_gives_100() {
        let mut cs = ClosingStrength::new("cs", 3).unwrap();
        for _ in 0..3 {
            cs.update_bar(&bar("15", "5", "15")).unwrap(); // close=high → 100%
        }
        let v = cs.update_bar(&bar("15", "5", "15")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_close_at_low_gives_zero() {
        let mut cs = ClosingStrength::new("cs", 3).unwrap();
        for _ in 0..3 {
            cs.update_bar(&bar("15", "5", "5")).unwrap(); // close=low → 0%
        }
        let v = cs.update_bar(&bar("15", "5", "5")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut cs = ClosingStrength::new("cs", 2).unwrap();
        cs.update_bar(&bar("12", "10", "11")).unwrap();
        cs.update_bar(&bar("12", "10", "11")).unwrap();
        assert!(cs.is_ready());
        cs.reset();
        assert!(!cs.is_ready());
    }
}
