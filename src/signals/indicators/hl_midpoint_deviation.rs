//! High-Low Midpoint Deviation indicator.
//!
//! Measures how far the closing price deviates from the bar's midpoint,
//! normalized by the bar's range. Detects bars where the close settled
//! away from the equilibrium midpoint.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling EMA of `(close - midpoint) / range`.
///
/// The raw deviation for each bar is:
/// ```text
/// raw = (close - (high + low) / 2) / (high - low)   when high > low
///     = 0                                            when high == low (flat bar)
/// ```
///
/// This ranges from `-0.5` (close at low, furthest below midpoint) to
/// `+0.5` (close at high, furthest above midpoint).
///
/// The EMA smooths this signal across `period` bars. Positive values
/// indicate persistent closing above the midpoint (bullish commitment);
/// negative values indicate persistent closing below (bearish commitment).
///
/// Returns a value after the first bar (EMA seeds with first raw value).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HlMidpointDeviation;
/// use fin_primitives::signals::Signal;
///
/// let hmd = HlMidpointDeviation::new("hmd", 10).unwrap();
/// assert_eq!(hmd.period(), 10);
/// ```
pub struct HlMidpointDeviation {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl HlMidpointDeviation {
    /// Constructs a new `HlMidpointDeviation`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::from(2u32) / (Decimal::from(period as u32) + Decimal::ONE);
        Ok(Self { name: name.into(), period, ema: None, k })
    }
}

impl Signal for HlMidpointDeviation {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.ema.is_some()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let raw = if range.is_zero() {
            Decimal::ZERO
        } else {
            let mid = bar.midpoint();
            (bar.close - mid)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        let ema = match self.ema {
            None => {
                self.ema = Some(raw);
                raw
            }
            Some(prev) => {
                let next = raw * self.k + prev * (Decimal::ONE - self.k);
                self.ema = Some(next);
                next
            }
        };

        Ok(SignalValue::Scalar(ema))
    }

    fn reset(&mut self) {
        self.ema = None;
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
    fn test_hmd_invalid_period() {
        assert!(HlMidpointDeviation::new("hmd", 0).is_err());
    }

    #[test]
    fn test_hmd_ready_after_first_bar() {
        let mut hmd = HlMidpointDeviation::new("hmd", 5).unwrap();
        hmd.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(hmd.is_ready());
    }

    #[test]
    fn test_hmd_close_at_midpoint_zero() {
        let mut hmd = HlMidpointDeviation::new("hmd", 5).unwrap();
        // mid = 100; close = 100; raw = 0; EMA seeds at 0
        let v = hmd.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hmd_close_at_high_positive() {
        let mut hmd = HlMidpointDeviation::new("hmd", 5).unwrap();
        // close at high: raw = (110 - 100) / 20 = 0.5
        let v = hmd.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_hmd_close_at_low_negative() {
        let mut hmd = HlMidpointDeviation::new("hmd", 5).unwrap();
        // close at low: raw = (90 - 100) / 20 = -0.5
        let v = hmd.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-0.5)));
    }

    #[test]
    fn test_hmd_flat_bar_zero() {
        let mut hmd = HlMidpointDeviation::new("hmd", 5).unwrap();
        let v = hmd.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hmd_reset() {
        let mut hmd = HlMidpointDeviation::new("hmd", 5).unwrap();
        hmd.update_bar(&bar("110", "90", "105")).unwrap();
        assert!(hmd.is_ready());
        hmd.reset();
        assert!(!hmd.is_ready());
    }

    #[test]
    fn test_hmd_persistent_upper_closes_positive() {
        let mut hmd = HlMidpointDeviation::new("hmd", 3).unwrap();
        for _ in 0..5 {
            hmd.update_bar(&bar("110", "90", "110")).unwrap();
        }
        let v = hmd.update_bar(&bar("110", "90", "110")).unwrap();
        if let SignalValue::Scalar(e) = v {
            assert!(e > dec!(0), "persistent upper closes → positive EMA: {e}");
        } else {
            panic!("expected Scalar");
        }
    }
}
