//! High-Low Ratio Moving Average indicator.
//!
//! Tracks the rolling moving average of (high / low) ratios, providing a
//! smooth measure of intrabar spread expansion and contraction.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Low Ratio Moving Average: rolling SMA of `high / low`.
///
/// Each bar's high-to-low ratio measures the spread multiplier — how many
/// times the high exceeds the low. A ratio of `1.0` is a flat bar; larger
/// values indicate wider intrabar spreads (higher volatility). The SMA
/// smooths this across `period` bars.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been
/// accumulated or when any bar has `low == 0`.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowRatioMa;
/// use fin_primitives::signals::Signal;
///
/// let hlr = HighLowRatioMa::new("hlr", 10).unwrap();
/// assert_eq!(hlr.period(), 10);
/// assert!(!hlr.is_ready());
/// ```
pub struct HighLowRatioMa {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl HighLowRatioMa {
    /// Constructs a new `HighLowRatioMa`.
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
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for HighLowRatioMa {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.low.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = bar
            .high
            .checked_div(bar.low)
            .ok_or(FinError::ArithmeticOverflow)?;

        self.sum += ratio;
        self.window.push_back(ratio);

        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        #[allow(clippy::cast_possible_truncation)]
        let n = Decimal::from(self.period as u32);
        let mean = self.sum.checked_div(n).ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(mean))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(high: &str, low: &str) -> OhlcvBar {
        let h = Price::new(high.parse().unwrap()).unwrap();
        let l = Price::new(low.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l,
            high: h,
            low: l,
            close: h,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_hlrma_invalid_period() {
        assert!(HighLowRatioMa::new("hlr", 0).is_err());
    }

    #[test]
    fn test_hlrma_unavailable_during_warmup() {
        let mut hlr = HighLowRatioMa::new("hlr", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(hlr.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_hlrma_flat_bars_ratio_one() {
        let mut hlr = HighLowRatioMa::new("hlr", 3).unwrap();
        for _ in 0..3 {
            hlr.update_bar(&bar("100", "100")).unwrap();
        }
        let v = hlr.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_hlrma_ratio_above_one_for_wide_bar() {
        let mut hlr = HighLowRatioMa::new("hlr", 2).unwrap();
        hlr.update_bar(&bar("110", "90")).unwrap();
        let v = hlr.update_bar(&bar("110", "90")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(1), "high/low ratio should be > 1: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hlrma_reset() {
        let mut hlr = HighLowRatioMa::new("hlr", 3).unwrap();
        for _ in 0..3 {
            hlr.update_bar(&bar("110", "90")).unwrap();
        }
        assert!(hlr.is_ready());
        hlr.reset();
        assert!(!hlr.is_ready());
    }
}
