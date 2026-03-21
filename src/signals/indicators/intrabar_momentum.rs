//! Intrabar Momentum indicator.
//!
//! Measures the net directional movement within a bar relative to its opening
//! price, normalized by the bar's range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Intrabar Momentum: rolling mean of `(close - open) / range`.
///
/// Each bar's raw intrabar momentum is `(close - open) / (high - low)`,
/// which ranges from `-1` (opened at high, closed at low) to `+1` (opened at
/// low, closed at high), with `0` for flat-range bars (treated as zero).
///
/// The rolling mean over `period` bars smooths this into a trend signal:
///
/// - **> 0**: on average, bars close in the upper half of their range → bullish pressure.
/// - **< 0**: on average, bars close in the lower half → bearish pressure.
/// - **≈ 0**: balanced intrabar momentum.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::IntrabarMomentum;
/// use fin_primitives::signals::Signal;
///
/// let im = IntrabarMomentum::new("intrabar_mom", 10).unwrap();
/// assert_eq!(im.period(), 10);
/// assert!(!im.is_ready());
/// ```
pub struct IntrabarMomentum {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl IntrabarMomentum {
    /// Constructs a new `IntrabarMomentum`.
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

impl Signal for IntrabarMomentum {
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
        let range = bar.range();
        let raw = if range.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - bar.open)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.sum += raw;
        self.window.push_back(raw);

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

    fn bar(open: &str, high: &str, low: &str, close: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(open.parse().unwrap()).unwrap(),
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
    fn test_ibm_invalid_period() {
        assert!(IntrabarMomentum::new("ibm", 0).is_err());
    }

    #[test]
    fn test_ibm_unavailable_during_warmup() {
        let mut im = IntrabarMomentum::new("im", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(
                im.update_bar(&bar("100", "110", "90", "105")).unwrap(),
                SignalValue::Unavailable
            );
        }
    }

    #[test]
    fn test_ibm_all_bullish_positive() {
        let mut im = IntrabarMomentum::new("im", 3).unwrap();
        // open at low, close at high → raw = +1 each bar
        for _ in 0..3 {
            im.update_bar(&bar("90", "110", "90", "110")).unwrap();
        }
        let v = im.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ibm_all_bearish_negative() {
        let mut im = IntrabarMomentum::new("im", 3).unwrap();
        // open at high, close at low → raw = -1 each bar
        for _ in 0..3 {
            im.update_bar(&bar("110", "110", "90", "90")).unwrap();
        }
        let v = im.update_bar(&bar("110", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_ibm_flat_bar_zero() {
        let mut im = IntrabarMomentum::new("im", 2).unwrap();
        for _ in 0..2 {
            im.update_bar(&bar("100", "100", "100", "100")).unwrap();
        }
        let v = im.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ibm_reset() {
        let mut im = IntrabarMomentum::new("im", 3).unwrap();
        for _ in 0..3 {
            im.update_bar(&bar("100", "110", "90", "105")).unwrap();
        }
        assert!(im.is_ready());
        im.reset();
        assert!(!im.is_ready());
    }
}
