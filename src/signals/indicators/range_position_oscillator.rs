//! Range Position Oscillator indicator.
//!
//! Computes the difference between a fast EMA and slow EMA of the Close Location
//! Value (CLV), providing a MACD-like momentum signal based on where the close
//! settles within the bar's range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Difference between fast-EMA(CLV) and slow-EMA(CLV).
///
/// The Close Location Value (CLV) for each bar is:
/// ```text
/// clv = (close - low - (high - close)) / (high - low)
///     = (2*close - high - low) / (high - low)
/// ```
///
/// This ranges from `-1` (close at low) to `+1` (close at high).
///
/// The oscillator is:
/// ```text
/// rpo = EMA(clv, fast_period) - EMA(clv, slow_period)
/// ```
///
/// Positive values indicate the fast-term CLV trend is above the slow-term
/// (bullish bias strengthening). Negative indicates weakening.
///
/// Returns a value after the first bar (both EMAs seed immediately). `period()` returns `slow_period`.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if either period is 0, or `fast_period >= slow_period`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangePositionOscillator;
/// use fin_primitives::signals::Signal;
///
/// let rpo = RangePositionOscillator::new("rpo", 5, 20).unwrap();
/// assert_eq!(rpo.period(), 20);
/// ```
pub struct RangePositionOscillator {
    name: String,
    slow_period: usize,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    fast_k: Decimal,
    slow_k: Decimal,
}

impl RangePositionOscillator {
    /// Constructs a new `RangePositionOscillator`.
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
        #[allow(clippy::cast_possible_truncation)]
        let fast_k = Decimal::from(2u32) / (Decimal::from(fast_period as u32) + Decimal::ONE);
        #[allow(clippy::cast_possible_truncation)]
        let slow_k = Decimal::from(2u32) / (Decimal::from(slow_period as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            slow_period,
            fast_ema: None,
            slow_ema: None,
            fast_k,
            slow_k,
        })
    }
}

impl crate::signals::Signal for RangePositionOscillator {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.slow_period
    }

    fn is_ready(&self) -> bool {
        self.fast_ema.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let clv = bar.close_location_value();

        let fast = match self.fast_ema {
            None => { self.fast_ema = Some(clv); clv }
            Some(prev) => {
                let next = clv * self.fast_k + prev * (Decimal::ONE - self.fast_k);
                self.fast_ema = Some(next);
                next
            }
        };

        let slow = match self.slow_ema {
            None => { self.slow_ema = Some(clv); clv }
            Some(prev) => {
                let next = clv * self.slow_k + prev * (Decimal::ONE - self.slow_k);
                self.slow_ema = Some(next);
                next
            }
        };

        Ok(SignalValue::Scalar(fast - slow))
    }

    fn reset(&mut self) {
        self.fast_ema = None;
        self.slow_ema = None;
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
    fn test_rpo_invalid_period() {
        assert!(RangePositionOscillator::new("rpo", 0, 20).is_err());
        assert!(RangePositionOscillator::new("rpo", 5, 0).is_err());
        assert!(RangePositionOscillator::new("rpo", 20, 5).is_err());
        assert!(RangePositionOscillator::new("rpo", 5, 5).is_err());
    }

    #[test]
    fn test_rpo_ready_after_first_bar() {
        let mut rpo = RangePositionOscillator::new("rpo", 5, 20).unwrap();
        rpo.update_bar(&bar("110", "90", "110")).unwrap();
        assert!(rpo.is_ready());
    }

    #[test]
    fn test_rpo_seeds_zero_on_first_bar() {
        // First bar: fast==slow (both seed to same CLV) → difference = 0
        let mut rpo = RangePositionOscillator::new("rpo", 5, 20).unwrap();
        let v = rpo.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rpo_bullish_bars_eventually_positive() {
        let mut rpo = RangePositionOscillator::new("rpo", 3, 10).unwrap();
        // Seed with neutral bar then flood with bullish bars
        rpo.update_bar(&bar("110", "90", "100")).unwrap();
        for _ in 0..20 {
            rpo.update_bar(&bar("110", "90", "110")).unwrap();
        }
        let v = rpo.update_bar(&bar("110", "90", "110")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0), "persistent bullish → fast > slow: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rpo_reset() {
        let mut rpo = RangePositionOscillator::new("rpo", 5, 20).unwrap();
        rpo.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(rpo.is_ready());
        rpo.reset();
        assert!(!rpo.is_ready());
    }
}
