//! High-Close Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Close Ratio.
///
/// Measures how close the closing price is to the session high, rolling over `period` bars.
/// A consistently high ratio indicates bullish sentiment — price closes near the top.
///
/// Per-bar formula: `hcr = close / high` (if high > 0, else 0)
///
/// Rolling: `mean(hcr, period)`
///
/// - 1.0: close consistently equals the high (maximum bullish close).
/// - < 1.0: close regularly pulls back from the high.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighCloseRatio;
/// use fin_primitives::signals::Signal;
/// let hcr = HighCloseRatio::new("hcr_14", 14).unwrap();
/// assert_eq!(hcr.period(), 14);
/// ```
pub struct HighCloseRatio {
    name: String,
    period: usize,
    ratios: VecDeque<Decimal>,
}

impl HighCloseRatio {
    /// Constructs a new `HighCloseRatio`.
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

impl Signal for HighCloseRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ratio = if bar.high.is_zero() {
            Decimal::ZERO
        } else {
            bar.close.checked_div(bar.high).ok_or(FinError::ArithmeticOverflow)?
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

    fn bar(h: &str, c: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let lo = Price::new("1".parse().unwrap()).unwrap();
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
        assert!(matches!(HighCloseRatio::new("hcr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut hcr = HighCloseRatio::new("hcr", 3).unwrap();
        assert_eq!(hcr.update_bar(&bar("12", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_close_equals_high_gives_one() {
        let mut hcr = HighCloseRatio::new("hcr", 3).unwrap();
        for _ in 0..3 {
            hcr.update_bar(&bar("15", "15")).unwrap();
        }
        let v = hcr.update_bar(&bar("15", "15")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_close_below_high() {
        let mut hcr = HighCloseRatio::new("hcr", 3).unwrap();
        for _ in 0..3 {
            hcr.update_bar(&bar("20", "10")).unwrap(); // close/high=0.5
        }
        let v = hcr.update_bar(&bar("20", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_reset() {
        let mut hcr = HighCloseRatio::new("hcr", 2).unwrap();
        hcr.update_bar(&bar("12", "11")).unwrap();
        hcr.update_bar(&bar("12", "11")).unwrap();
        assert!(hcr.is_ready());
        hcr.reset();
        assert!(!hcr.is_ready());
    }
}
