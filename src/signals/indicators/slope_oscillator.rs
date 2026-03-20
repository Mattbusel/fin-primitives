//! Slope Oscillator — measures acceleration of an EMA as the change in its slope.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Slope Oscillator — the bar-over-bar change in an EMA's per-bar slope.
///
/// This is a second-derivative measure: it answers "is the moving average
/// speeding up or slowing down?" Positive values indicate the EMA is
/// accelerating upward; negative values indicate deceleration or reversal.
///
/// Algorithm:
/// 1. Compute `EMA(period)` on each bar.
/// 2. Compute the slope as `ema_now - ema_prev`.
/// 3. Output `slope_now - slope_prev` (the change in slope = EMA acceleration).
///
/// Returns [`SignalValue::Unavailable`] for the first two bars after the EMA warms up
/// (two slopes are needed to compute a change).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SlopeOscillator;
/// use fin_primitives::signals::Signal;
/// let so = SlopeOscillator::new("so_10", 10).unwrap();
/// assert_eq!(so.period(), 10);
/// ```
pub struct SlopeOscillator {
    name: String,
    period: usize,
    k: Decimal,
    ema: Option<Decimal>,
    prev_ema: Option<Decimal>,
    prev_slope: Option<Decimal>,
    bars_seen: usize,
    ready: bool,
}

impl SlopeOscillator {
    /// Constructs a new `SlopeOscillator`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::from(2u32)
            .checked_div(Decimal::from(period as u32 + 1))
            .unwrap_or(Decimal::ONE);
        Ok(Self {
            name: name.into(),
            period,
            k,
            ema: None,
            prev_ema: None,
            prev_slope: None,
            bars_seen: 0,
            ready: false,
        })
    }
}

impl Signal for SlopeOscillator {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bars_seen += 1;

        // Update EMA.
        let new_ema = match self.ema {
            None => bar.close,
            Some(prev) => {
                let one_minus_k = Decimal::ONE - self.k;
                bar.close * self.k + prev * one_minus_k
            }
        };

        // Compute current slope = ema_now - ema_prev.
        let slope = match self.prev_ema {
            None => {
                self.prev_ema = Some(new_ema);
                self.ema = Some(new_ema);
                return Ok(SignalValue::Unavailable);
            }
            Some(pe) => new_ema - pe,
        };

        self.prev_ema = Some(new_ema);
        self.ema = Some(new_ema);

        // Compute acceleration = slope_now - slope_prev.
        let acceleration = match self.prev_slope {
            None => {
                self.prev_slope = Some(slope);
                return Ok(SignalValue::Unavailable);
            }
            Some(ps) => slope - ps,
        };
        self.prev_slope = Some(slope);
        self.ready = true;

        Ok(SignalValue::Scalar(acceleration))
    }

    fn reset(&mut self) {
        self.ema = None;
        self.prev_ema = None;
        self.prev_slope = None;
        self.bars_seen = 0;
        self.ready = false;
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
    fn test_so_invalid_period() {
        assert!(SlopeOscillator::new("so", 0).is_err());
    }

    #[test]
    fn test_so_unavailable_for_first_two_bars() {
        let mut so = SlopeOscillator::new("so", 5).unwrap();
        assert_eq!(so.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(so.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!so.is_ready());
    }

    #[test]
    fn test_so_produces_value_on_third_bar() {
        let mut so = SlopeOscillator::new("so", 3).unwrap();
        so.update_bar(&bar("100")).unwrap();
        so.update_bar(&bar("102")).unwrap();
        let v = so.update_bar(&bar("104")).unwrap();
        assert!(v.is_scalar());
        assert!(so.is_ready());
    }

    #[test]
    fn test_so_flat_series_zero_acceleration() {
        let mut so = SlopeOscillator::new("so", 1).unwrap();
        // Period-1 EMA is just each close; slope = ema - prev_ema = 0; acceleration = 0.
        so.update_bar(&bar("100")).unwrap();
        so.update_bar(&bar("100")).unwrap();
        let v = so.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_so_reset() {
        let mut so = SlopeOscillator::new("so", 3).unwrap();
        for _ in 0..5 {
            so.update_bar(&bar("100")).unwrap();
        }
        assert!(so.is_ready());
        so.reset();
        assert!(!so.is_ready());
        assert_eq!(so.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_so_period_and_name() {
        let so = SlopeOscillator::new("my_so", 10).unwrap();
        assert_eq!(so.period(), 10);
        assert_eq!(so.name(), "my_so");
    }
}
