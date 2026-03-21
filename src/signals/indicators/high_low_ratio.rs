//! High-Low Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Low Ratio.
///
/// Computes the ratio of the rolling period high to the rolling period low.
/// This gives a measure of the overall price dispersion over the window.
///
/// Formula: `hlr = period_high / period_low`
///
/// - Values near 1.0: tight range (consolidation).
/// - High values: wide price spread (trending or volatile).
/// - Returns 0 when period_low is zero.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowRatio;
/// use fin_primitives::signals::Signal;
/// let hlr = HighLowRatio::new("hlr_20", 20).unwrap();
/// assert_eq!(hlr.period(), 20);
/// ```
pub struct HighLowRatio {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl HighLowRatio {
    /// Constructs a new `HighLowRatio`.
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
        })
    }
}

impl Signal for HighLowRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);

        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let period_high = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let period_low = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);

        if period_low.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let ratio = period_high.checked_div(period_low).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: hi,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(HighLowRatio::new("hlr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut hlr = HighLowRatio::new("hlr", 3).unwrap();
        assert_eq!(hlr.update_bar(&bar("12", "10")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ratio_calculation() {
        // All same: high=120, low=100 → ratio=1.2
        let mut hlr = HighLowRatio::new("hlr", 3).unwrap();
        for _ in 0..3 {
            hlr.update_bar(&bar("120", "100")).unwrap();
        }
        let v = hlr.update_bar(&bar("120", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1.2)));
    }

    #[test]
    fn test_same_high_low_gives_one() {
        // All bars with same high=low → ratio=1
        let mut hlr = HighLowRatio::new("hlr", 3).unwrap();
        for _ in 0..3 {
            hlr.update_bar(&bar("100", "100")).unwrap();
        }
        let v = hlr.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_reset() {
        let mut hlr = HighLowRatio::new("hlr", 2).unwrap();
        hlr.update_bar(&bar("12", "10")).unwrap();
        hlr.update_bar(&bar("12", "10")).unwrap();
        assert!(hlr.is_ready());
        hlr.reset();
        assert!(!hlr.is_ready());
    }
}
