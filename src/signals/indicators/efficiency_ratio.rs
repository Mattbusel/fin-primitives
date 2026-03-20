//! Kaufman Efficiency Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Efficiency Ratio (ER) — measures how directionally price is moving.
///
/// ```text
/// direction = |close_t − close_{t−period}|
/// path      = Σ |close_i − close_{i−1}|  (sum of bar-to-bar changes over period)
/// ER        = direction / path
/// ```
///
/// Values near 1.0 indicate strongly trending; near 0 indicate choppy/random.
/// Returns 0 when path is zero (flat market).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EfficiencyRatio;
/// use fin_primitives::signals::Signal;
///
/// let er = EfficiencyRatio::new("er", 10).unwrap();
/// assert_eq!(er.period(), 10);
/// ```
pub struct EfficiencyRatio {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl EfficiencyRatio {
    /// Creates a new `EfficiencyRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for EfficiencyRatio {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 { self.closes.pop_front(); }
        if self.closes.len() < self.period + 1 { return Ok(SignalValue::Unavailable); }

        let closes: Vec<&Decimal> = self.closes.iter().collect();
        let first = *closes[0];
        let last = *closes[self.period];

        let direction = (last - first).abs();

        let path: Decimal = closes.windows(2)
            .map(|w| (*w[1] - *w[0]).abs())
            .sum();

        if path.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        Ok(SignalValue::Scalar(direction / path))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.closes.clear();
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
    fn test_er_invalid() {
        assert!(EfficiencyRatio::new("e", 0).is_err());
    }

    #[test]
    fn test_er_unavailable_before_warmup() {
        let mut e = EfficiencyRatio::new("e", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(e.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_er_trending_is_one() {
        // Perfect trend: each bar moves same direction → ER = 1
        let mut e = EfficiencyRatio::new("e", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for p in ["100", "101", "102", "103"] {
            last = e.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_er_flat_is_zero() {
        // Flat market: path > 0 only if prices vary; flat → both zero → ER = 0
        let mut e = EfficiencyRatio::new("e", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = e.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_er_choppy_less_than_one() {
        // Zigzag: path > direction → ER < 1
        let mut e = EfficiencyRatio::new("e", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for p in ["100", "105", "100", "105"] {
            last = e.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v < dec!(1), "expected ER < 1, got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_er_range_0_to_1() {
        let mut e = EfficiencyRatio::new("e", 3).unwrap();
        for p in ["100", "105", "95", "102", "98", "110", "88", "103"] {
            if let SignalValue::Scalar(v) = e.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "out of range: {v}");
            }
        }
    }

    #[test]
    fn test_er_reset() {
        let mut e = EfficiencyRatio::new("e", 3).unwrap();
        for p in ["100", "101", "102", "103"] { e.update_bar(&bar(p)).unwrap(); }
        assert!(e.is_ready());
        e.reset();
        assert!(!e.is_ready());
    }
}
