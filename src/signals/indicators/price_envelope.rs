//! Price Envelope indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Envelope — a fixed-percentage band above and below a simple moving average.
///
/// ```text
/// SMA_t  = SMA(close, period)
/// upper  = SMA_t × (1 + pct/100)
/// lower  = SMA_t × (1 − pct/100)
/// output = (close - SMA_t) / SMA_t × 100   (position within envelope, in %)
/// ```
///
/// Positive values mean close is above the SMA; negative means below.
/// Values near ±`pct` indicate the close is near the envelope boundary.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceEnvelope;
/// use fin_primitives::signals::Signal;
///
/// let pe = PriceEnvelope::new("pe", 20, "2.5".parse().unwrap()).unwrap();
/// assert_eq!(pe.period(), 20);
/// ```
pub struct PriceEnvelope {
    name: String,
    period: usize,
    pct: Decimal,
    closes: VecDeque<Decimal>,
    upper: Option<Decimal>,
    lower: Option<Decimal>,
}

impl PriceEnvelope {
    /// Creates a new `PriceEnvelope`.
    ///
    /// - `pct`: band width as a percentage of the SMA (e.g. `2.5` for ±2.5%).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if `pct` is not positive.
    pub fn new(name: impl Into<String>, period: usize, pct: Decimal) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        if pct <= Decimal::ZERO {
            return Err(FinError::InvalidInput("pct must be positive".into()));
        }
        Ok(Self {
            name: name.into(),
            period,
            pct,
            closes: VecDeque::with_capacity(period),
            upper: None,
            lower: None,
        })
    }

    /// Returns the current upper envelope level.
    pub fn upper(&self) -> Option<Decimal> { self.upper }
    /// Returns the current lower envelope level.
    pub fn lower(&self) -> Option<Decimal> { self.lower }
}

impl Signal for PriceEnvelope {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }
        if self.closes.len() < self.period { return Ok(SignalValue::Unavailable); }

        #[allow(clippy::cast_possible_truncation)]
        let sma = self.closes.iter().sum::<Decimal>() / Decimal::from(self.period as u32);
        let factor = self.pct / Decimal::from(100u32);
        self.upper = Some(sma * (Decimal::ONE + factor));
        self.lower = Some(sma * (Decimal::ONE - factor));

        if sma.is_zero() { return Ok(SignalValue::Scalar(Decimal::ZERO)); }
        Ok(SignalValue::Scalar((bar.close - sma) / sma * Decimal::from(100u32)))
    }

    fn is_ready(&self) -> bool { self.upper.is_some() }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.closes.clear();
        self.upper = None;
        self.lower = None;
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
    fn test_pe_invalid() {
        assert!(PriceEnvelope::new("p", 0, dec!(2.5)).is_err());
        assert!(PriceEnvelope::new("p", 20, dec!(0)).is_err());
        assert!(PriceEnvelope::new("p", 20, dec!(-1)).is_err());
    }

    #[test]
    fn test_pe_flat_is_zero() {
        let mut pe = PriceEnvelope::new("p", 3, dec!(2.5)).unwrap();
        pe.update_bar(&bar("100")).unwrap();
        pe.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = pe.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pe_bands_set() {
        let mut pe = PriceEnvelope::new("p", 3, dec!(2)).unwrap();
        for _ in 0..3 { pe.update_bar(&bar("100")).unwrap(); }
        assert_eq!(pe.upper(), Some(dec!(102)));
        assert_eq!(pe.lower(), Some(dec!(98)));
    }

    #[test]
    fn test_pe_reset() {
        let mut pe = PriceEnvelope::new("p", 3, dec!(2)).unwrap();
        for _ in 0..3 { pe.update_bar(&bar("100")).unwrap(); }
        assert!(pe.is_ready());
        pe.reset();
        assert!(!pe.is_ready());
    }
}
