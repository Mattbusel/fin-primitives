//! Volatility Skew indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Volatility Skew — asymmetry between upside and downside volatility.
///
/// ```text
/// up_changes   = {|close_t − close_{t−1}| : close_t > close_{t−1}}
/// down_changes = {|close_t − close_{t−1}| : close_t < close_{t−1}}
///
/// up_vol   = mean(up_changes)    or 0 if no up bars
/// down_vol = mean(down_changes)  or 0 if no down bars
///
/// output = (up_vol − down_vol) / (up_vol + down_vol) × 100
/// ```
///
/// Positive output indicates larger upside moves (positive skew / upward momentum).
/// Negative indicates larger downside moves (negative skew / downward momentum).
/// Returns 0 when both up_vol and down_vol are zero (flat/no changes).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilitySkew;
/// use fin_primitives::signals::Signal;
///
/// let vs = VolatilitySkew::new("vs", 20).unwrap();
/// assert_eq!(vs.period(), 20);
/// ```
pub struct VolatilitySkew {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    changes: VecDeque<(Decimal, bool)>, // (abs_change, is_up)
}

impl VolatilitySkew {
    /// Creates a new `VolatilitySkew`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            changes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolatilitySkew {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if bar.close != pc {
                let abs_change = (bar.close - pc).abs();
                let is_up = bar.close > pc;
                self.changes.push_back((abs_change, is_up));
                if self.changes.len() > self.period { self.changes.pop_front(); }
            }
        }
        self.prev_close = Some(bar.close);

        if self.changes.len() < self.period { return Ok(SignalValue::Unavailable); }

        let mut up_sum = Decimal::ZERO;
        let mut up_count = 0u32;
        let mut down_sum = Decimal::ZERO;
        let mut down_count = 0u32;

        for &(change, is_up) in &self.changes {
            if is_up {
                up_sum += change;
                up_count += 1;
            } else {
                down_sum += change;
                down_count += 1;
            }
        }

        let up_vol = if up_count > 0 {
            up_sum.to_f64().unwrap_or(0.0) / up_count as f64
        } else {
            0.0
        };
        let down_vol = if down_count > 0 {
            down_sum.to_f64().unwrap_or(0.0) / down_count as f64
        } else {
            0.0
        };

        let total = up_vol + down_vol;
        if total == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let skew = (up_vol - down_vol) / total * 100.0;
        Ok(SignalValue::Scalar(
            Decimal::try_from(skew).unwrap_or(Decimal::ZERO)
        ))
    }

    fn is_ready(&self) -> bool { self.changes.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.prev_close = None;
        self.changes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
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
    fn test_vs_invalid() {
        assert!(VolatilitySkew::new("v", 0).is_err());
    }

    #[test]
    fn test_vs_unavailable_before_warmup() {
        let mut v = VolatilitySkew::new("v", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vs_all_up_is_100() {
        // Only up changes → down_vol = 0, up_vol > 0 → skew = 100
        let mut v = VolatilitySkew::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..7 {
            let p = format!("{}", 100 + i * 5);
            last = v.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vs_all_down_is_minus_100() {
        let mut v = VolatilitySkew::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..7 {
            let p = format!("{}", 200 - i * 5);
            last = v.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(-100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vs_symmetric_is_zero() {
        // Equal up/down moves of same magnitude: skew = 0
        let mut v = VolatilitySkew::new("v", 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            v.update_bar(&bar("100")).unwrap();
            v.update_bar(&bar("110")).unwrap();
            v.update_bar(&bar("110")).unwrap();
            last = v.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(val) = last {
            let diff = val.abs();
            assert!(diff < dec!(0.001), "expected ~0, got {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vs_range_minus_100_to_100() {
        let mut v = VolatilitySkew::new("v", 4).unwrap();
        for price in ["100", "105", "98", "107", "95", "110", "88", "112", "90"] {
            if let SignalValue::Scalar(val) = v.update_bar(&bar(price)).unwrap() {
                assert!(val >= dec!(-100) && val <= dec!(100), "out of range: {val}");
            }
        }
    }

    #[test]
    fn test_vs_reset() {
        let mut v = VolatilitySkew::new("v", 3).unwrap();
        for i in 0u32..7 {
            let p = format!("{}", 100 + i * 5);
            v.update_bar(&bar(&p)).unwrap();
        }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
