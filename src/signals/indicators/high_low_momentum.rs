//! High-Low Momentum indicator.
//!
//! Tracks the rolling SMA of the average change in both high and low per bar,
//! measuring the composite momentum across the entire bar structure rather than
//! just the close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling SMA of `((high[t] - high[t-1]) + (low[t] - low[t-1])) / 2`.
///
/// Each bar's raw contribution measures how much both the high and low have
/// shifted from the previous bar, averaged together. Positive values indicate
/// the entire price structure is drifting upward; negative values indicate
/// downward drift.
///
/// Returns [`SignalValue::Unavailable`] until `period` deltas have accumulated
/// (requires `period + 1` bars total).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowMomentum;
/// use fin_primitives::signals::Signal;
///
/// let hlm = HighLowMomentum::new("hlm", 10).unwrap();
/// assert_eq!(hlm.period(), 10);
/// assert!(!hlm.is_ready());
/// ```
pub struct HighLowMomentum {
    name: String,
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl HighLowMomentum {
    /// Constructs a new `HighLowMomentum`.
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
            prev_high: None,
            prev_low: None,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for HighLowMomentum {
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
        let result = if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            let delta_h = bar.high - ph;
            let delta_l = bar.low - pl;
            let raw = (delta_h + delta_l)
                .checked_div(Decimal::TWO)
                .ok_or(FinError::ArithmeticOverflow)?;

            self.sum += raw;
            self.window.push_back(raw);

            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }

            if self.window.len() < self.period {
                SignalValue::Unavailable
            } else {
                #[allow(clippy::cast_possible_truncation)]
                let mean = self.sum
                    .checked_div(Decimal::from(self.period as u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                SignalValue::Scalar(mean)
            }
        } else {
            SignalValue::Unavailable
        };

        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
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
    fn test_hlm_invalid_period() {
        assert!(HighLowMomentum::new("hlm", 0).is_err());
    }

    #[test]
    fn test_hlm_unavailable_during_warmup() {
        let mut hlm = HighLowMomentum::new("hlm", 3).unwrap();
        assert_eq!(hlm.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(hlm.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(hlm.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_hlm_constant_bars_zero() {
        // No change in high/low → all deltas 0 → SMA = 0
        let mut hlm = HighLowMomentum::new("hlm", 3).unwrap();
        for _ in 0..4 {
            hlm.update_bar(&bar("110", "90")).unwrap();
        }
        let v = hlm.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hlm_trending_up_positive() {
        // Each bar: high +2, low +2 → delta per bar = 2 → SMA = 2
        let mut hlm = HighLowMomentum::new("hlm", 3).unwrap();
        let mut h = 110u32;
        let mut l = 90u32;
        for _ in 0..4 {
            hlm.update_bar(&bar(&h.to_string(), &l.to_string())).unwrap();
            h += 2;
            l += 2;
        }
        let v = hlm.update_bar(&bar(&h.to_string(), &l.to_string())).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert_eq!(s, dec!(2), "trending up should give SMA=2: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hlm_reset() {
        let mut hlm = HighLowMomentum::new("hlm", 3).unwrap();
        for _ in 0..5 {
            hlm.update_bar(&bar("110", "90")).unwrap();
        }
        assert!(hlm.is_ready());
        hlm.reset();
        assert!(!hlm.is_ready());
    }
}
