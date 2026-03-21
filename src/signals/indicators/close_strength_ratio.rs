//! Close Strength Ratio indicator.
//!
//! Compares the close's distance from the period low to the full period range,
//! producing a smoothed measure of relative price strength.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Strength Ratio: `(close - period_low) / (period_high - period_low)`.
///
/// Measures where the current close sits within the `period`-bar high-low range:
/// - `1.0`: close at the period's high → maximum bullish strength.
/// - `0.0`: close at the period's low → maximum bearish weakness.
/// - `0.5`: close at the midpoint of the period range.
///
/// Returns zero when `period_high == period_low` (flat range). Ready after
/// `period` bars have been accumulated.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseStrengthRatio;
/// use fin_primitives::signals::Signal;
///
/// let csr = CloseStrengthRatio::new("csr", 14).unwrap();
/// assert_eq!(csr.period(), 14);
/// assert!(!csr.is_ready());
/// ```
pub struct CloseStrengthRatio {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl CloseStrengthRatio {
    /// Constructs a new `CloseStrengthRatio`.
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

impl Signal for CloseStrengthRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period
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
        let period_range = period_high - period_low;

        if period_range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let ratio = (bar.close - period_low)
            .checked_div(period_range)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(ratio))
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

    fn bar(high: &str, low: &str, close: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(low.parse().unwrap()).unwrap(),
            high: Price::new(high.parse().unwrap()).unwrap(),
            low: Price::new(low.parse().unwrap()).unwrap(),
            close: Price::new(close.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_csr_invalid_period() {
        assert!(CloseStrengthRatio::new("csr", 0).is_err());
    }

    #[test]
    fn test_csr_unavailable_during_warmup() {
        let mut csr = CloseStrengthRatio::new("csr", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(csr.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_csr_close_at_period_high_returns_one() {
        let mut csr = CloseStrengthRatio::new("csr", 3).unwrap();
        csr.update_bar(&bar("100", "90", "95")).unwrap();
        csr.update_bar(&bar("100", "90", "95")).unwrap();
        // period high=110, low=90, range=20; close=110 → ratio=1
        let v = csr.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_csr_close_at_period_low_returns_zero() {
        let mut csr = CloseStrengthRatio::new("csr", 3).unwrap();
        csr.update_bar(&bar("110", "90", "100")).unwrap();
        csr.update_bar(&bar("110", "90", "100")).unwrap();
        // period high=110, low=90, range=20; close=90 → ratio=0
        let v = csr.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_csr_flat_range_returns_zero() {
        let mut csr = CloseStrengthRatio::new("csr", 3).unwrap();
        for _ in 0..3 {
            csr.update_bar(&bar("100", "100", "100")).unwrap();
        }
        let v = csr.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_csr_in_range() {
        let mut csr = CloseStrengthRatio::new("csr", 3).unwrap();
        csr.update_bar(&bar("110", "90", "100")).unwrap();
        csr.update_bar(&bar("110", "90", "100")).unwrap();
        let v = csr.update_bar(&bar("110", "90", "100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r >= dec!(0) && r <= dec!(1), "ratio out of [0,1]: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_csr_reset() {
        let mut csr = CloseStrengthRatio::new("csr", 3).unwrap();
        for _ in 0..3 {
            csr.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(csr.is_ready());
        csr.reset();
        assert!(!csr.is_ready());
    }
}
