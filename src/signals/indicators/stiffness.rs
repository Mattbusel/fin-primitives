//! Stiffness indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Stiffness — percentage of recent closes that lie above their own SMA.
///
/// ```text
/// SMA_t   = SMA(close, sma_period)
/// above_t = 1 if close_t > SMA_t, else 0
/// output  = count(above, count_period) / count_period × 100
/// ```
///
/// Values near 100 indicate a strong trend; near 0 indicate persistent weakness.
/// Requires `sma_period + count_period − 1` bars to warm up.
///
/// Returns [`SignalValue::Unavailable`] until enough bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Stiffness;
/// use fin_primitives::signals::Signal;
///
/// let s = Stiffness::new("stiff", 20, 60).unwrap();
/// assert_eq!(s.period(), 20);
/// ```
pub struct Stiffness {
    name: String,
    sma_period: usize,
    count_period: usize,
    closes: VecDeque<Decimal>,
    above: VecDeque<u8>,
}

impl Stiffness {
    /// Creates a new `Stiffness`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero.
    pub fn new(
        name: impl Into<String>,
        sma_period: usize,
        count_period: usize,
    ) -> Result<Self, FinError> {
        if sma_period == 0 { return Err(FinError::InvalidPeriod(sma_period)); }
        if count_period == 0 { return Err(FinError::InvalidPeriod(count_period)); }
        Ok(Self {
            name: name.into(),
            sma_period,
            count_period,
            closes: VecDeque::with_capacity(sma_period),
            above: VecDeque::with_capacity(count_period),
        })
    }
}

impl Signal for Stiffness {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.sma_period { self.closes.pop_front(); }
        if self.closes.len() < self.sma_period { return Ok(SignalValue::Unavailable); }

        #[allow(clippy::cast_possible_truncation)]
        let sma = self.closes.iter().sum::<Decimal>() / Decimal::from(self.sma_period as u32);
        let flag: u8 = if bar.close > sma { 1 } else { 0 };

        self.above.push_back(flag);
        if self.above.len() > self.count_period { self.above.pop_front(); }
        if self.above.len() < self.count_period { return Ok(SignalValue::Unavailable); }

        let count = self.above.iter().map(|&v| u32::from(v)).sum::<u32>();
        #[allow(clippy::cast_possible_truncation)]
        let pct = Decimal::from(count) / Decimal::from(self.count_period as u32)
            * Decimal::from(100u32);
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.above.len() >= self.count_period }
    fn period(&self) -> usize { self.sma_period }

    fn reset(&mut self) {
        self.closes.clear();
        self.above.clear();
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
    fn test_stiffness_invalid() {
        assert!(Stiffness::new("s", 0, 10).is_err());
        assert!(Stiffness::new("s", 5, 0).is_err());
    }

    #[test]
    fn test_stiffness_unavailable_during_warmup() {
        let mut s = Stiffness::new("s", 3, 4).unwrap();
        // needs sma_period + count_period - 1 = 6 bars; bars 1-5 are Unavailable
        for _ in 0..5 {
            assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        // bar 6 produces the first Scalar
        assert!(matches!(s.update_bar(&bar("100")).unwrap(), SignalValue::Scalar(_)));
    }

    #[test]
    fn test_stiffness_all_above_is_100() {
        // Rising prices: every close will be above a SMA of lower values
        // Use sma=2, count=3; feed: 100,101,102,103,104 — each close > SMA
        let mut s = Stiffness::new("s", 2, 3).unwrap();
        let prices = ["100", "101", "102", "103", "104"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = s.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(100));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_stiffness_all_below_is_zero() {
        // Falling prices: 100, 99, 98, 97, 96 — each close < SMA
        let mut s = Stiffness::new("s", 2, 3).unwrap();
        let prices = ["100", "99", "98", "97", "96"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = s.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_stiffness_reset() {
        let mut s = Stiffness::new("s", 2, 3).unwrap();
        for _ in 0..10 { s.update_bar(&bar("100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
