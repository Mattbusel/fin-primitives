//! Price Above MA indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Above MA — percentage of recent closes that lie above a longer-period SMA.
///
/// ```text
/// SMA_long = SMA(close, long_period)
/// count    = number of closes in last `short_period` bars above SMA_long
/// output   = count / short_period × 100
/// ```
///
/// Values near 100 indicate persistent strength; near 0 indicate persistent weakness.
/// Requires `long_period + short_period − 1` bars to warm up.
///
/// Returns [`SignalValue::Unavailable`] until fully warmed up.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceAboveMa;
/// use fin_primitives::signals::Signal;
///
/// let pa = PriceAboveMa::new("pama", 200, 20).unwrap();
/// assert_eq!(pa.period(), 200);
/// ```
pub struct PriceAboveMa {
    name: String,
    long_period: usize,
    short_period: usize,
    long_closes: VecDeque<Decimal>,
    above_flags: VecDeque<u8>,
}

impl PriceAboveMa {
    /// Creates a new `PriceAboveMa`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero or `short >= long`.
    pub fn new(
        name: impl Into<String>,
        long_period: usize,
        short_period: usize,
    ) -> Result<Self, FinError> {
        if long_period == 0 { return Err(FinError::InvalidPeriod(long_period)); }
        if short_period == 0 { return Err(FinError::InvalidPeriod(short_period)); }
        if short_period >= long_period { return Err(FinError::InvalidPeriod(short_period)); }
        Ok(Self {
            name: name.into(),
            long_period,
            short_period,
            long_closes: VecDeque::with_capacity(long_period),
            above_flags: VecDeque::with_capacity(short_period),
        })
    }
}

impl Signal for PriceAboveMa {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.long_closes.push_back(bar.close);
        if self.long_closes.len() > self.long_period { self.long_closes.pop_front(); }
        if self.long_closes.len() < self.long_period { return Ok(SignalValue::Unavailable); }

        let sma = self.long_closes.iter().sum::<Decimal>()
            / Decimal::from(self.long_period as u32);
        let flag: u8 = if bar.close > sma { 1 } else { 0 };

        self.above_flags.push_back(flag);
        if self.above_flags.len() > self.short_period { self.above_flags.pop_front(); }
        if self.above_flags.len() < self.short_period { return Ok(SignalValue::Unavailable); }

        let count = self.above_flags.iter().map(|&v| u32::from(v)).sum::<u32>();
        let pct = Decimal::from(count)
            / Decimal::from(self.short_period as u32)
            * Decimal::from(100u32);
        Ok(SignalValue::Scalar(pct))
    }

    fn is_ready(&self) -> bool { self.above_flags.len() >= self.short_period }
    fn period(&self) -> usize { self.long_period }

    fn reset(&mut self) {
        self.long_closes.clear();
        self.above_flags.clear();
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
    fn test_pama_invalid() {
        assert!(PriceAboveMa::new("p", 0, 5).is_err());
        assert!(PriceAboveMa::new("p", 10, 0).is_err());
        assert!(PriceAboveMa::new("p", 5, 10).is_err()); // short >= long
        assert!(PriceAboveMa::new("p", 10, 10).is_err()); // short >= long
    }

    #[test]
    fn test_pama_unavailable_before_warmup() {
        let mut p = PriceAboveMa::new("p", 5, 3).unwrap();
        assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!p.is_ready());
    }

    #[test]
    fn test_pama_all_above_is_100() {
        // Rising prices: every close will be above the long SMA
        let mut p = PriceAboveMa::new("p", 3, 2).unwrap();
        let prices = ["100", "101", "102", "103", "104"];
        let mut last = SignalValue::Unavailable;
        for price in &prices { last = p.update_bar(&bar(price)).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pama_all_below_is_zero() {
        // Falling prices: closes drop below the long SMA
        let mut p = PriceAboveMa::new("p", 3, 2).unwrap();
        // Warm up with high prices, then drop
        for _ in 0..3 { p.update_bar(&bar("200")).unwrap(); }
        // Now feed low prices — they'll be below SMA of 200s
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 { last = p.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pama_reset() {
        let mut p = PriceAboveMa::new("p", 3, 2).unwrap();
        for _ in 0..10 { p.update_bar(&bar("100")).unwrap(); }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
