//! Support/Resistance Distance indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Support/Resistance Distance — distance of close from the N-period high and low.
///
/// ```text
/// resistance = max(high, period)
/// support    = min(low,  period)
/// mid        = (resistance + support) / 2
/// output     = (close − mid) / (resistance − support) × 100
/// ```
///
/// Values near +50 mean close is near resistance; near -50 near support; 0 at midpoint.
/// Returns 0 when the range is zero (flat market).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SupportResistanceDistance;
/// use fin_primitives::signals::Signal;
///
/// let sr = SupportResistanceDistance::new("sr", 20).unwrap();
/// assert_eq!(sr.period(), 20);
/// ```
pub struct SupportResistanceDistance {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl SupportResistanceDistance {
    /// Creates a new `SupportResistanceDistance`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }

    /// Returns the current resistance level (N-period high).
    pub fn resistance(&self) -> Option<Decimal> {
        if self.highs.len() < self.period { None }
        else { self.highs.iter().cloned().max() }
    }

    /// Returns the current support level (N-period low).
    pub fn support(&self) -> Option<Decimal> {
        if self.lows.len() < self.period { None }
        else { self.lows.iter().cloned().min() }
    }
}

impl Signal for SupportResistanceDistance {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period { self.highs.pop_front(); }
        if self.lows.len() > self.period { self.lows.pop_front(); }
        if self.highs.len() < self.period { return Ok(SignalValue::Unavailable); }

        let resistance = self.highs.iter().cloned().max().unwrap();
        let support = self.lows.iter().cloned().min().unwrap();
        let range = resistance - support;

        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let mid = (resistance + support) / Decimal::from(2u32);
        let dist = (bar.close - mid) / range * Decimal::from(100u32);
        Ok(SignalValue::Scalar(dist))
    }

    fn is_ready(&self) -> bool { self.highs.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hlc(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn bar(c: &str) -> OhlcvBar { bar_hlc(c, c, c) }

    #[test]
    fn test_sr_invalid() {
        assert!(SupportResistanceDistance::new("s", 0).is_err());
    }

    #[test]
    fn test_sr_unavailable_before_warmup() {
        let mut s = SupportResistanceDistance::new("s", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_sr_flat_is_zero() {
        let mut s = SupportResistanceDistance::new("s", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = s.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_sr_at_resistance_near_50() {
        // range=[90..110], close=110 → mid=100, dist=(110-100)/20*100=50
        let mut s = SupportResistanceDistance::new("s", 3).unwrap();
        s.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        s.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar_hlc("110", "110", "110")).unwrap() {
            assert_eq!(v, dec!(50));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_sr_at_support_near_minus_50() {
        let mut s = SupportResistanceDistance::new("s", 3).unwrap();
        s.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        s.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar_hlc("90", "90", "90")).unwrap() {
            assert_eq!(v, dec!(-50));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_sr_accessors() {
        let mut s = SupportResistanceDistance::new("s", 2).unwrap();
        s.update_bar(&bar_hlc("110", "90", "100")).unwrap();
        s.update_bar(&bar_hlc("115", "85", "100")).unwrap();
        assert_eq!(s.resistance(), Some(dec!(115)));
        assert_eq!(s.support(), Some(dec!(85)));
    }

    #[test]
    fn test_sr_reset() {
        let mut s = SupportResistanceDistance::new("s", 3).unwrap();
        for _ in 0..5 { s.update_bar(&bar("100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
        assert!(s.resistance().is_none());
    }
}
