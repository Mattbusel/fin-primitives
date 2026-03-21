//! Tail Asymmetry indicator.
//!
//! Rolling EMA of the ratio of upper shadow to lower shadow, measuring
//! where the close sits relative to the bar's extremes.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Tail Asymmetry — EMA of `(high - close) / (close - low)`.
///
/// For each bar:
/// ```text
/// upper_shadow = high - close
/// lower_shadow = close - low
/// tail_ratio   = upper_shadow / lower_shadow   when lower_shadow > 0
///              = 1                              when close == low (flat lower shadow)
/// ```
///
/// - **> 1**: upper shadow exceeds lower — price closed near the low, sellers
///   dominated the session. Bearish pressure.
/// - **< 1**: lower shadow exceeds upper — price closed near the high, buyers
///   dominated. Bullish pressure.
/// - **= 1**: close sits at the midpoint of the bar's range.
///
/// The EMA smooths bar-to-bar noise to reveal persistent shadow structure.
/// Returns a value from the first bar.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TailAsymmetry;
/// use fin_primitives::signals::Signal;
/// let ta = TailAsymmetry::new("ta_14", 14).unwrap();
/// assert_eq!(ta.period(), 14);
/// ```
pub struct TailAsymmetry {
    name: String,
    period: usize,
    ema: Option<Decimal>,
    k: Decimal,
}

impl TailAsymmetry {
    /// Constructs a new `TailAsymmetry`.
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

impl Signal for TailAsymmetry {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.ema.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let upper = bar.high - bar.close;
        let lower = bar.close - bar.low;

        let raw = if lower.is_zero() {
            Decimal::ONE
        } else {
            upper.checked_div(lower).ok_or(FinError::ArithmeticOverflow)?
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

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ta_invalid_period() {
        assert!(TailAsymmetry::new("ta", 0).is_err());
    }

    #[test]
    fn test_ta_ready_after_first_bar() {
        let mut ta = TailAsymmetry::new("ta", 5).unwrap();
        ta.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert!(ta.is_ready());
    }

    #[test]
    fn test_ta_close_at_midpoint_one() {
        // high=110, low=90, close=100 → upper=10, lower=10 → ratio=1
        let mut ta = TailAsymmetry::new("ta", 5).unwrap();
        let v = ta.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ta_close_near_high_below_one() {
        // high=110, low=90, close=108 → upper=2, lower=18 → ratio < 1 (bullish)
        let mut ta = TailAsymmetry::new("ta", 5).unwrap();
        if let SignalValue::Scalar(v) = ta.update_bar(&bar("100", "110", "90", "108")).unwrap() {
            assert!(v < dec!(1), "close near high → ratio < 1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ta_close_near_low_above_one() {
        // high=110, low=90, close=92 → upper=18, lower=2 → ratio > 1 (bearish)
        let mut ta = TailAsymmetry::new("ta", 5).unwrap();
        if let SignalValue::Scalar(v) = ta.update_bar(&bar("100", "110", "90", "92")).unwrap() {
            assert!(v > dec!(1), "close near low → ratio > 1: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ta_flat_lower_shadow_returns_one() {
        // close == low → lower_shadow = 0 → returns 1
        let mut ta = TailAsymmetry::new("ta", 5).unwrap();
        let v = ta.update_bar(&bar("95", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ta_reset() {
        let mut ta = TailAsymmetry::new("ta", 5).unwrap();
        ta.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert!(ta.is_ready());
        ta.reset();
        assert!(!ta.is_ready());
    }
}
