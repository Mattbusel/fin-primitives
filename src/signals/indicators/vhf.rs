//! Vertical Horizontal Filter (VHF).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Vertical Horizontal Filter — distinguishes trending from choppy markets.
///
/// `VHF = (highest_close - lowest_close) / Σ|close[i] - close[i-1]|`
///
/// High VHF (> 0.4) → trending market.
/// Low VHF (< 0.2)  → choppy / sideways market.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Vhf;
/// use fin_primitives::signals::Signal;
///
/// let v = Vhf::new("vhf14", 14).unwrap();
/// assert_eq!(v.period(), 14);
/// assert!(!v.is_ready());
/// ```
pub struct Vhf {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl Vhf {
    /// Constructs a new `Vhf`.
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

impl Signal for Vhf {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let mut path = Decimal::ZERO;
        let closes_vec: Vec<Decimal> = self.closes.iter().copied().collect();
        for w in closes_vec.windows(2) {
            path += (w[1] - w[0]).abs();
        }
        if path.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let hi = closes_vec.iter().copied().fold(Decimal::MIN, Decimal::max);
        let lo = closes_vec.iter().copied().fold(Decimal::MAX, Decimal::min);
        Ok(SignalValue::Scalar((hi - lo) / path))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }

    fn period(&self) -> usize { self.period }

    fn reset(&mut self) { self.closes.clear(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
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
    fn test_vhf_invalid_period() { assert!(Vhf::new("v", 0).is_err()); }

    #[test]
    fn test_vhf_unavailable_before_period() {
        let mut v = Vhf::new("v", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vhf_trending_high() {
        // Perfectly monotone rise → VHF = 1.0
        let mut v = Vhf::new("v", 4).unwrap();
        for i in 0..5 {
            let p = format!("{}", 100 + i * 10);
            v.update_bar(&bar(&p)).unwrap();
        }
        match v.update_bar(&bar("150")).unwrap() {
            SignalValue::Scalar(val) => assert!(val > dec!(0.5), "trending VHF should be high: {val}"),
            _ => panic!("expected scalar"),
        }
    }

    #[test]
    fn test_vhf_flat_is_zero() {
        let mut v = Vhf::new("v", 3).unwrap();
        for _ in 0..4 { v.update_bar(&bar("100")).unwrap(); }
        assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Scalar(Decimal::ZERO));
    }

    #[test]
    fn test_vhf_reset() {
        let mut v = Vhf::new("v", 3).unwrap();
        for i in 0..4 { let p = format!("{}", 100+i); v.update_bar(&bar(&p)).unwrap(); }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
