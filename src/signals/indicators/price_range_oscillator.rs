//! Price Range Oscillator indicator.
//!
//! Computes the difference between a fast SMA of the bar range and a slow SMA
//! of the bar range, identifying periods of expanding or contracting volatility.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Fast SMA(range, fast_period) minus Slow SMA(range, slow_period).
///
/// Positive values indicate the recent short-term bar range is larger than the
/// longer-term average — volatility is expanding. Negative values indicate
/// short-term ranges are below the longer-term average — volatility is
/// contracting or compressing.
///
/// Returns [`SignalValue::Unavailable`] until `slow_period` bars have been seen.
/// `period()` returns `slow_period`.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if either period is 0 or `fast >= slow`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceRangeOscillator;
/// use fin_primitives::signals::Signal;
///
/// let pro = PriceRangeOscillator::new("pro", 5, 20).unwrap();
/// assert_eq!(pro.period(), 20);
/// assert!(!pro.is_ready());
/// ```
pub struct PriceRangeOscillator {
    name: String,
    fast_period: usize,
    slow_period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceRangeOscillator {
    /// Constructs a new `PriceRangeOscillator`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if periods are 0 or `fast >= slow`.
    pub fn new(
        name: impl Into<String>,
        fast_period: usize,
        slow_period: usize,
    ) -> Result<Self, FinError> {
        if fast_period == 0 {
            return Err(FinError::InvalidPeriod(fast_period));
        }
        if slow_period == 0 || fast_period >= slow_period {
            return Err(FinError::InvalidPeriod(slow_period));
        }
        Ok(Self {
            name: name.into(),
            fast_period,
            slow_period,
            window: VecDeque::with_capacity(slow_period),
            sum: Decimal::ZERO,
        })
    }
}

impl crate::signals::Signal for PriceRangeOscillator {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.slow_period
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.slow_period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();

        self.sum += range;
        self.window.push_back(range);

        if self.window.len() > self.slow_period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }

        if self.window.len() < self.slow_period {
            return Ok(SignalValue::Unavailable);
        }

        // slow SMA = sum / slow_period (window is exactly slow_period here)
        #[allow(clippy::cast_possible_truncation)]
        let slow_sma = self.sum
            .checked_div(Decimal::from(self.slow_period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        // fast SMA = sum of last fast_period entries / fast_period
        let fast_sum: Decimal = self.window
            .iter()
            .rev()
            .take(self.fast_period)
            .copied()
            .fold(Decimal::ZERO, |a, b| a + b);

        #[allow(clippy::cast_possible_truncation)]
        let fast_sma = fast_sum
            .checked_div(Decimal::from(self.fast_period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(fast_sma - slow_sma))
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
            open: l, high: h, low: l, close: h,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pro_invalid_period() {
        assert!(PriceRangeOscillator::new("pro", 0, 20).is_err());
        assert!(PriceRangeOscillator::new("pro", 5, 0).is_err());
        assert!(PriceRangeOscillator::new("pro", 20, 5).is_err());
        assert!(PriceRangeOscillator::new("pro", 5, 5).is_err());
    }

    #[test]
    fn test_pro_unavailable_during_warmup() {
        let mut pro = PriceRangeOscillator::new("pro", 3, 6).unwrap();
        for _ in 0..5 {
            assert_eq!(pro.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pro_constant_range_zero() {
        // Fast and slow SMA of same range → difference = 0
        let mut pro = PriceRangeOscillator::new("pro", 3, 6).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..8 {
            last = pro.update_bar(&bar("110", "90")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pro_expanding_ranges_positive() {
        // Fill with narrow bars then switch to wide
        let mut pro = PriceRangeOscillator::new("pro", 2, 5).unwrap();
        // Seed with range=10 bars
        for _ in 0..5 {
            pro.update_bar(&bar("105", "95")).unwrap();
        }
        // Now add wide bars; fast SMA should exceed slow SMA
        pro.update_bar(&bar("150", "50")).unwrap();
        let v = pro.update_bar(&bar("150", "50")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0), "expanding ranges → positive oscillator: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pro_reset() {
        let mut pro = PriceRangeOscillator::new("pro", 3, 6).unwrap();
        for _ in 0..7 {
            pro.update_bar(&bar("110", "90")).unwrap();
        }
        assert!(pro.is_ready());
        pro.reset();
        assert!(!pro.is_ready());
    }
}
