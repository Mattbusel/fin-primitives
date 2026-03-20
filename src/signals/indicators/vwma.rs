//! Volume-Weighted Moving Average (VWMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume-Weighted Moving Average over `period` bars.
///
/// Each close price is weighted by the bar's volume. Bars with higher volume
/// contribute more to the average, making VWMA more responsive to high-activity
/// periods than a simple SMA.
///
/// ```text
/// VWMA = Σ(close_i * volume_i) / Σ(volume_i)
/// ```
///
/// Returns `SignalValue::Unavailable` until `period` bars have been seen,
/// or if the total volume in the window is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Vwma;
/// use fin_primitives::signals::Signal;
/// let vwma = Vwma::new("vwma_10", 10).unwrap();
/// assert_eq!(vwma.period(), 10);
/// ```
pub struct Vwma {
    name: String,
    period: usize,
    /// Rolling window of (close, volume) pairs.
    window: VecDeque<(Decimal, Decimal)>,
}

impl Vwma {
    /// Constructs a new `Vwma` with the given name and period.
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
        })
    }
}

impl Signal for Vwma {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back((bar.close, bar.volume));
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mut pv_sum = Decimal::ZERO;
        let mut vol_sum = Decimal::ZERO;
        for &(close, volume) in &self.window {
            pv_sum += close * volume;
            vol_sum += volume;
        }

        if vol_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let vwma = pv_sum.checked_div(vol_sum).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(vwma))
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: Decimal, volume: Decimal) -> OhlcvBar {
        let p = Price::new(close).unwrap();
        let q = Quantity::new(volume).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: q,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vwma_period_0_fails() {
        assert!(Vwma::new("v", 0).is_err());
    }

    #[test]
    fn test_vwma_unavailable_before_period() {
        let mut v = Vwma::new("v2", 2).unwrap();
        assert_eq!(v.update_bar(&bar(dec!(10), dec!(100))).unwrap(), SignalValue::Unavailable);
        assert!(!v.is_ready());
    }

    #[test]
    fn test_vwma_equal_volumes_equals_sma() {
        // With equal volumes, VWMA == SMA
        let mut v = Vwma::new("v3", 3).unwrap();
        v.update_bar(&bar(dec!(10), dec!(1))).unwrap();
        v.update_bar(&bar(dec!(20), dec!(1))).unwrap();
        let result = v.update_bar(&bar(dec!(30), dec!(1))).unwrap();
        // SMA = 20, VWMA should also = 20
        assert_eq!(result, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_vwma_high_volume_bar_dominates() {
        // Bar 3 has much higher volume → VWMA closer to bar 3's price
        let mut v = Vwma::new("v3", 3).unwrap();
        v.update_bar(&bar(dec!(10), dec!(1))).unwrap();
        v.update_bar(&bar(dec!(10), dec!(1))).unwrap();
        let result = match v.update_bar(&bar(dec!(100), dec!(100))).unwrap() {
            SignalValue::Scalar(x) => x,
            _ => panic!("expected scalar"),
        };
        // SMA = 40; VWMA should be ≫ 40 due to the high-volume bar
        assert!(result > dec!(40), "VWMA {result} should exceed SMA ≈ 40");
    }

    #[test]
    fn test_vwma_zero_volume_returns_unavailable() {
        let mut v = Vwma::new("v2", 2).unwrap();
        v.update_bar(&bar(dec!(10), dec!(0))).unwrap();
        let result = v.update_bar(&bar(dec!(20), dec!(0))).unwrap();
        assert_eq!(result, SignalValue::Unavailable);
    }

    #[test]
    fn test_vwma_reset() {
        let mut v = Vwma::new("v2", 2).unwrap();
        v.update_bar(&bar(dec!(10), dec!(1))).unwrap();
        v.update_bar(&bar(dec!(20), dec!(1))).unwrap();
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
        assert_eq!(v.update_bar(&bar(dec!(10), dec!(1))).unwrap(), SignalValue::Unavailable);
    }
}
