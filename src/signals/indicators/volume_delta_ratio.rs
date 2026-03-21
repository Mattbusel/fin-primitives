//! Volume Delta Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Delta Ratio.
///
/// Estimates the proportion of buying versus selling volume using price position
/// within the bar as a proxy. This approximates the "delta" (buy vol − sell vol)
/// without tick data.
///
/// Per-bar allocation:
/// - `buy_vol = volume * close_position` where `close_position = (close - low) / range`
/// - `sell_vol = volume * (1 - close_position)`
/// - `delta_ratio = (buy_vol - sell_vol) / total_vol = 2 * close_position - 1`
///
/// Rolling: `mean(delta_ratio, period)`
///
/// - +1.0: consistently closes at the high (maximum buying pressure).
/// - −1.0: consistently closes at the low (maximum selling pressure).
/// - Zero-range bars contribute 0 to the rolling average.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeDeltaRatio;
/// use fin_primitives::signals::Signal;
/// let vdr = VolumeDeltaRatio::new("vdr_14", 14).unwrap();
/// assert_eq!(vdr.period(), 14);
/// ```
pub struct VolumeDeltaRatio {
    name: String,
    period: usize,
    deltas: VecDeque<Decimal>,
}

impl VolumeDeltaRatio {
    /// Constructs a new `VolumeDeltaRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, deltas: VecDeque::with_capacity(period) })
    }
}

impl Signal for VolumeDeltaRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let delta = if range.is_zero() {
            Decimal::ZERO
        } else {
            // close_position ∈ [0, 1]
            let close_pos = (bar.close - bar.low)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?;
            // delta_ratio = 2 * close_pos - 1 ∈ [-1, 1]
            close_pos
                .checked_mul(Decimal::TWO)
                .ok_or(FinError::ArithmeticOverflow)?
                - Decimal::ONE
        };

        self.deltas.push_back(delta);
        if self.deltas.len() > self.period {
            self.deltas.pop_front();
        }
        if self.deltas.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.deltas.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.deltas.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.deltas.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(VolumeDeltaRatio::new("vdr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut vdr = VolumeDeltaRatio::new("vdr", 3).unwrap();
        assert_eq!(vdr.update_bar(&bar("12", "10", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_close_at_high_gives_one() {
        let mut vdr = VolumeDeltaRatio::new("vdr", 3).unwrap();
        for _ in 0..3 {
            vdr.update_bar(&bar("12", "10", "12")).unwrap(); // close=high → +1
        }
        let v = vdr.update_bar(&bar("12", "10", "12")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_close_at_low_gives_neg_one() {
        let mut vdr = VolumeDeltaRatio::new("vdr", 3).unwrap();
        for _ in 0..3 {
            vdr.update_bar(&bar("12", "10", "10")).unwrap(); // close=low → -1
        }
        let v = vdr.update_bar(&bar("12", "10", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_close_at_midpoint_gives_zero() {
        let mut vdr = VolumeDeltaRatio::new("vdr", 3).unwrap();
        for _ in 0..3 {
            vdr.update_bar(&bar("12", "10", "11")).unwrap(); // close=mid → 0
        }
        let v = vdr.update_bar(&bar("12", "10", "11")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut vdr = VolumeDeltaRatio::new("vdr", 2).unwrap();
        vdr.update_bar(&bar("12", "10", "11")).unwrap();
        vdr.update_bar(&bar("12", "10", "11")).unwrap();
        assert!(vdr.is_ready());
        vdr.reset();
        assert!(!vdr.is_ready());
    }
}
