//! Median Price indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Median Price — rolling median of (high + low) / 2 over a window.
///
/// ```text
/// typical_i  = (high_i + low_i) / 2
/// output     = median(typical, period)
/// ```
///
/// More robust to outliers than a simple moving average of the midpoint.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MedianPrice;
/// use fin_primitives::signals::Signal;
///
/// let mp = MedianPrice::new("mp", 5).unwrap();
/// assert_eq!(mp.period(), 5);
/// ```
pub struct MedianPrice {
    name: String,
    period: usize,
    midpoints: VecDeque<Decimal>,
}

impl MedianPrice {
    /// Creates a new `MedianPrice`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            midpoints: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for MedianPrice {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = (bar.high + bar.low) / Decimal::from(2u32);
        self.midpoints.push_back(mid);
        if self.midpoints.len() > self.period { self.midpoints.pop_front(); }
        if self.midpoints.len() < self.period { return Ok(SignalValue::Unavailable); }

        let mut sorted: Vec<Decimal> = self.midpoints.iter().cloned().collect();
        sorted.sort();

        let median = if self.period % 2 == 1 {
            sorted[self.period / 2]
        } else {
            (sorted[self.period / 2 - 1] + sorted[self.period / 2]) / Decimal::from(2u32)
        };

        Ok(SignalValue::Scalar(median))
    }

    fn is_ready(&self) -> bool { self.midpoints.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.midpoints.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hl(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(dec!(100)).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn bar(c: &str) -> OhlcvBar { bar_hl(c, c) }

    #[test]
    fn test_mp_invalid() {
        assert!(MedianPrice::new("m", 0).is_err());
    }

    #[test]
    fn test_mp_unavailable_before_warmup() {
        let mut m = MedianPrice::new("m", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(m.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_mp_odd_period_median() {
        // midpoints: 90, 100, 110 → sorted: [90,100,110] → median = 100
        let mut m = MedianPrice::new("m", 3).unwrap();
        m.update_bar(&bar_hl("90", "90")).unwrap();
        m.update_bar(&bar_hl("100", "100")).unwrap();
        if let SignalValue::Scalar(v) = m.update_bar(&bar_hl("110", "110")).unwrap() {
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_mp_even_period_median() {
        // midpoints: 90, 100, 110, 120 with period=4 → median = (100+110)/2 = 105
        let mut m = MedianPrice::new("m", 4).unwrap();
        m.update_bar(&bar_hl("90", "90")).unwrap();
        m.update_bar(&bar_hl("100", "100")).unwrap();
        m.update_bar(&bar_hl("110", "110")).unwrap();
        if let SignalValue::Scalar(v) = m.update_bar(&bar_hl("120", "120")).unwrap() {
            assert_eq!(v, dec!(105));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_mp_hl_midpoint() {
        // bar with h=110, l=90 → midpoint = 100; single bar period=1
        let mut m = MedianPrice::new("m", 1).unwrap();
        if let SignalValue::Scalar(v) = m.update_bar(&bar_hl("110", "90")).unwrap() {
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_mp_reset() {
        let mut m = MedianPrice::new("m", 3).unwrap();
        for _ in 0..5 { m.update_bar(&bar("100")).unwrap(); }
        assert!(m.is_ready());
        m.reset();
        assert!(!m.is_ready());
    }
}
