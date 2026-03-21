//! Median Price MA indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Median Price MA.
///
/// Rolling simple moving average of the median price `(high + low) / 2`.
/// A simple price reference that focuses on the bar's range midpoint
/// rather than the close.
///
/// Formula: `mp = (high + low) / 2`
///
/// Rolling: `mean(mp, period)`
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MedianPriceMa;
/// use fin_primitives::signals::Signal;
/// let mpm = MedianPriceMa::new("mpm_14", 14).unwrap();
/// assert_eq!(mpm.period(), 14);
/// ```
pub struct MedianPriceMa {
    name: String,
    period: usize,
    mps: VecDeque<Decimal>,
}

impl MedianPriceMa {
    /// Constructs a new `MedianPriceMa`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, mps: VecDeque::with_capacity(period) })
    }
}

impl Signal for MedianPriceMa {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mp = (bar.high + bar.low)
            .checked_div(Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;

        self.mps.push_back(mp);
        if self.mps.len() > self.period {
            self.mps.pop_front();
        }
        if self.mps.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.mps.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.mps.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.mps.clear();
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
        assert!(matches!(MedianPriceMa::new("mpm", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut mpm = MedianPriceMa::new("mpm", 3).unwrap();
        assert_eq!(mpm.update_bar(&bar("12", "8")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_known_median_price() {
        // high=12, low=8 → mp=10
        let mut mpm = MedianPriceMa::new("mpm", 3).unwrap();
        for _ in 0..3 {
            mpm.update_bar(&bar("12", "8")).unwrap();
        }
        let v = mpm.update_bar(&bar("12", "8")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_reset() {
        let mut mpm = MedianPriceMa::new("mpm", 2).unwrap();
        mpm.update_bar(&bar("12", "8")).unwrap();
        mpm.update_bar(&bar("12", "8")).unwrap();
        assert!(mpm.is_ready());
        mpm.reset();
        assert!(!mpm.is_ready());
    }
}
