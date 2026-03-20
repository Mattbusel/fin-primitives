//! Price Intensity indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Intensity — volume-weighted location of close within the bar range.
///
/// ```text
/// location_t = (2 × close_t − high_t − low_t) / (high_t − low_t)  [CLV, range −1..+1]
/// output     = mean(location × volume, period) / mean(volume, period)
/// ```
///
/// Positive output indicates buying pressure; negative selling pressure.
/// Returns 0 for doji bars (high == low) and when average volume is zero.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceIntensity;
/// use fin_primitives::signals::Signal;
///
/// let pi = PriceIntensity::new("pi", 14).unwrap();
/// assert_eq!(pi.period(), 14);
/// ```
pub struct PriceIntensity {
    name: String,
    period: usize,
    weighted: VecDeque<Decimal>,
    volumes: VecDeque<Decimal>,
}

impl PriceIntensity {
    /// Creates a new `PriceIntensity`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            weighted: VecDeque::with_capacity(period),
            volumes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for PriceIntensity {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let location = if range.is_zero() {
            Decimal::ZERO
        } else {
            (Decimal::from(2u32) * bar.close - bar.range()) / range
        };

        self.weighted.push_back(location * bar.volume);
        self.volumes.push_back(bar.volume);
        if self.weighted.len() > self.period { self.weighted.pop_front(); }
        if self.volumes.len() > self.period { self.volumes.pop_front(); }
        if self.weighted.len() < self.period { return Ok(SignalValue::Unavailable); }

        let vol_sum = self.volumes.iter().sum::<Decimal>();
        if vol_sum.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let weighted_sum = self.weighted.iter().sum::<Decimal>();
        Ok(SignalValue::Scalar(weighted_sum / vol_sum))
    }

    fn is_ready(&self) -> bool { self.weighted.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.weighted.clear();
        self.volumes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar_hlcv(h: &str, l: &str, c: &str, v: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let vol = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: vol,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn bar_p(c: &str) -> OhlcvBar { bar_hlcv(c, c, c, "1000") }

    #[test]
    fn test_pi_invalid() {
        assert!(PriceIntensity::new("p", 0).is_err());
    }

    #[test]
    fn test_pi_unavailable_before_warmup() {
        let mut p = PriceIntensity::new("p", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(p.update_bar(&bar_hlcv("110", "90", "100", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pi_close_at_high_is_one() {
        // close=high → location=1; uniform volume → PI = 1
        let mut p = PriceIntensity::new("p", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = p.update_bar(&bar_hlcv("110", "90", "110", "1000")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pi_close_at_low_is_minus_one() {
        let mut p = PriceIntensity::new("p", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = p.update_bar(&bar_hlcv("110", "90", "90", "1000")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(-1));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pi_doji_is_zero() {
        let mut p = PriceIntensity::new("p", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = p.update_bar(&bar_p("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pi_range_minus_one_to_one() {
        let mut p = PriceIntensity::new("p", 3).unwrap();
        for close in ["90", "110", "90", "110", "90", "110"] {
            if let SignalValue::Scalar(v) = p.update_bar(&bar_hlcv("110", "90", close, "1000")).unwrap() {
                assert!(v >= dec!(-1) && v <= dec!(1), "out of range: {v}");
            }
        }
    }

    #[test]
    fn test_pi_reset() {
        let mut p = PriceIntensity::new("p", 3).unwrap();
        for _ in 0..5 { p.update_bar(&bar_p("100")).unwrap(); }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
