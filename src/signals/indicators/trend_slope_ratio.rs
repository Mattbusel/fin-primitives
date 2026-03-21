//! Trend Slope Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Slope Ratio.
///
/// Compares the slope of two linear regression lines over different periods,
/// using a simplified slope approximation: `slope ≈ (close_t - close_{t-period}) / period`.
///
/// Formula:
/// - `short_slope = (close_t - close_{t-short}) / short`
/// - `long_slope = (close_t - close_{t-long}) / long`
/// - `ratio = short_slope / long_slope` (when long_slope != 0)
///
/// Interpretation:
/// - > 1.0: short-term slope steeper than long-term (momentum accelerating).
/// - 0 to 1.0: short-term slope shallower (decelerating).
/// - < 0: slopes diverging (potential reversal or consolidation).
/// - Returns 0.0 when long_slope is zero.
///
/// Returns `SignalValue::Unavailable` until `long_period + 1` closes accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendSlopeRatio;
/// use fin_primitives::signals::Signal;
/// let tsr = TrendSlopeRatio::new("tsr_5_20", 5, 20).unwrap();
/// assert_eq!(tsr.period(), 20);
/// ```
pub struct TrendSlopeRatio {
    name: String,
    short_period: usize,
    long_period: usize,
    closes: VecDeque<Decimal>,
}

impl TrendSlopeRatio {
    /// Constructs a new `TrendSlopeRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is 0 or `short_period >= long_period`.
    pub fn new(
        name: impl Into<String>,
        short_period: usize,
        long_period: usize,
    ) -> Result<Self, FinError> {
        if short_period == 0 {
            return Err(FinError::InvalidPeriod(short_period));
        }
        if long_period == 0 {
            return Err(FinError::InvalidPeriod(long_period));
        }
        if short_period >= long_period {
            return Err(FinError::InvalidPeriod(short_period));
        }
        Ok(Self {
            name: name.into(),
            short_period,
            long_period,
            closes: VecDeque::with_capacity(long_period + 1),
        })
    }
}

impl Signal for TrendSlopeRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.long_period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.long_period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let current = *self.closes.back().unwrap();
        let short_base = self.closes[self.closes.len() - 1 - self.short_period];
        let long_base = *self.closes.front().unwrap();

        #[allow(clippy::cast_possible_truncation)]
        let short_slope = (current - short_base)
            .checked_div(Decimal::from(self.short_period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        #[allow(clippy::cast_possible_truncation)]
        let long_slope = (current - long_base)
            .checked_div(Decimal::from(self.long_period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if long_slope.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let ratio = short_slope.checked_div(long_slope).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.long_period + 1
    }

    fn period(&self) -> usize {
        self.long_period
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
    fn test_invalid_params() {
        assert!(TrendSlopeRatio::new("tsr", 0, 10).is_err());
        assert!(TrendSlopeRatio::new("tsr", 10, 5).is_err());
        assert!(TrendSlopeRatio::new("tsr", 5, 5).is_err());
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut tsr = TrendSlopeRatio::new("tsr", 2, 5).unwrap();
        assert_eq!(tsr.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_flat_prices_zero() {
        let mut tsr = TrendSlopeRatio::new("tsr", 2, 4).unwrap();
        for _ in 0..5 {
            tsr.update_bar(&bar("100")).unwrap();
        }
        let v = tsr.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_linear_rise_gives_one() {
        // Linear rise: each step +2. short_slope = long_slope → ratio = 1
        let mut tsr = TrendSlopeRatio::new("tsr", 2, 4).unwrap();
        for i in 0..=4u32 {
            tsr.update_bar(&bar(&(100 + i * 2).to_string())).unwrap();
        }
        let v = tsr.update_bar(&bar("110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_reset() {
        let mut tsr = TrendSlopeRatio::new("tsr", 2, 4).unwrap();
        for _ in 0..5 {
            tsr.update_bar(&bar("100")).unwrap();
        }
        assert!(tsr.is_ready());
        tsr.reset();
        assert!(!tsr.is_ready());
    }
}
