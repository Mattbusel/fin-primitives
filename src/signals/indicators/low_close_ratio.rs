//! Low-Close Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Low-Close Ratio.
///
/// Measures the ratio of the closing price to the session low, rolling over `period` bars.
/// Complement of the High-Close Ratio. A consistently high ratio indicates bearish sentiment
/// — price closes far from the low (meaning it closed near the high).
/// Conversely, a low ratio (close to 1.0) means price often closes near the session low.
///
/// Per-bar formula: `lcr = low / close` (if close > 0, else 0)
///
/// Rolling: `mean(lcr, period)`
///
/// - Near 1.0: close consistently near or at the low (bearish close pattern).
/// - < 1.0: close well above the low (bullish close pattern).
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LowCloseRatio;
/// use fin_primitives::signals::Signal;
/// let lcr = LowCloseRatio::new("lcr_14", 14).unwrap();
/// assert_eq!(lcr.period(), 14);
/// ```
pub struct LowCloseRatio {
    name: String,
    period: usize,
    ratios: VecDeque<Decimal>,
}

impl LowCloseRatio {
    /// Constructs a new `LowCloseRatio`.
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

impl Signal for LowCloseRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ratio = if bar.close.is_zero() {
            Decimal::ZERO
        } else {
            bar.low.checked_div(bar.close).ok_or(FinError::ArithmeticOverflow)?
        };

        self.ratios.push_back(ratio);
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

    fn bar(l: &str, c: &str) -> OhlcvBar {
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let hi = Price::new("200".parse().unwrap()).unwrap();
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
        assert!(matches!(LowCloseRatio::new("lcr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut lcr = LowCloseRatio::new("lcr", 3).unwrap();
        assert_eq!(lcr.update_bar(&bar("10", "15")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_close_equals_low_gives_one() {
        let mut lcr = LowCloseRatio::new("lcr", 3).unwrap();
        for _ in 0..3 {
            lcr.update_bar(&bar("10", "10")).unwrap(); // low/close=1
        }
        let v = lcr.update_bar(&bar("10", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_close_well_above_low() {
        let mut lcr = LowCloseRatio::new("lcr", 3).unwrap();
        for _ in 0..3 {
            lcr.update_bar(&bar("10", "20")).unwrap(); // low/close=0.5
        }
        let v = lcr.update_bar(&bar("10", "20")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_reset() {
        let mut lcr = LowCloseRatio::new("lcr", 2).unwrap();
        lcr.update_bar(&bar("10", "15")).unwrap();
        lcr.update_bar(&bar("10", "15")).unwrap();
        assert!(lcr.is_ready());
        lcr.reset();
        assert!(!lcr.is_ready());
    }
}
