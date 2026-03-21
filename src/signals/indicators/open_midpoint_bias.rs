//! Open Midpoint Bias indicator.
//!
//! Tracks the EMA of `(open - midpoint) / range`, measuring whether the open
//! systematically gaps above or below the bar's equilibrium midpoint.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA of `(open − midpoint) / range` per bar.
///
/// For each bar the raw value is:
/// ```text
/// raw = (open - (high + low) / 2) / (high - low)   when high > low
///     = 0                                            when high == low (flat bar)
/// ```
///
/// Ranges from `-0.5` (open at low) to `+0.5` (open at high). A positive EMA
/// indicates the price repeatedly opens above the bar's center (gap-up bias);
/// negative indicates gap-down bias.
///
/// Returns a value after the first bar (EMA seeds with the first raw value).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenMidpointBias;
/// use fin_primitives::signals::Signal;
///
/// let omb = OpenMidpointBias::new("omb", 10).unwrap();
/// assert_eq!(omb.period(), 10);
/// ```
pub struct OpenMidpointBias {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl OpenMidpointBias {
    /// Constructs a new `OpenMidpointBias`.
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

impl crate::signals::Signal for OpenMidpointBias {
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
            (bar.open - mid)
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
    fn test_omb_invalid_period() {
        assert!(OpenMidpointBias::new("omb", 0).is_err());
    }

    #[test]
    fn test_omb_ready_after_first_bar() {
        let mut omb = OpenMidpointBias::new("omb", 5).unwrap();
        omb.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(omb.is_ready());
    }

    #[test]
    fn test_omb_open_at_midpoint_zero() {
        let mut omb = OpenMidpointBias::new("omb", 5).unwrap();
        // mid = (110+90)/2 = 100; open = 100 → raw = 0
        let v = omb.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_omb_open_at_high_positive() {
        let mut omb = OpenMidpointBias::new("omb", 5).unwrap();
        // open=110, mid=100, range=20 → raw = 10/20 = 0.5
        let v = omb.update_bar(&bar("110", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_omb_open_at_low_negative() {
        let mut omb = OpenMidpointBias::new("omb", 5).unwrap();
        // open=90, mid=100, range=20 → raw = -10/20 = -0.5
        let v = omb.update_bar(&bar("90", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-0.5)));
    }

    #[test]
    fn test_omb_flat_bar_zero() {
        let mut omb = OpenMidpointBias::new("omb", 5).unwrap();
        let v = omb.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_omb_reset() {
        let mut omb = OpenMidpointBias::new("omb", 5).unwrap();
        omb.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(omb.is_ready());
        omb.reset();
        assert!(!omb.is_ready());
    }
}
