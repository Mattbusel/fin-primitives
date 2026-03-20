//! Triangular Moving Average (TRIMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Triangular Moving Average — a double-smoothed SMA (SMA of SMA).
///
/// First computes an inner SMA over `period` bars, then takes an SMA of those
/// SMA values over the same `period`. This gives heavier weight to middle values,
/// producing a smoother but more lagged average than a simple SMA.
///
/// Requires `2 × period - 1` bars to produce the first value.
///
/// Returns [`SignalValue::Unavailable`] until enough bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Trima;
/// use fin_primitives::signals::Signal;
///
/// let mut t = Trima::new("trima10", 10).unwrap();
/// assert_eq!(t.period(), 10);
/// ```
pub struct Trima {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    sma_values: VecDeque<Decimal>,
}

impl Trima {
    /// Constructs a new `Trima`.
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
            closes: VecDeque::with_capacity(period),
            sma_values: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Trima {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }

        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // First-level SMA
        let sma: Decimal = self.closes.iter().sum::<Decimal>();
        #[allow(clippy::cast_possible_truncation)]
        let sma = sma / Decimal::from(self.period as u32);

        self.sma_values.push_back(sma);
        if self.sma_values.len() > self.period {
            self.sma_values.pop_front();
        }

        if self.sma_values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Second-level SMA of SMAs
        let trima: Decimal = self.sma_values.iter().sum::<Decimal>();
        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(trima / Decimal::from(self.period as u32)))
    }

    fn is_ready(&self) -> bool {
        self.sma_values.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.sma_values.clear();
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
    fn test_trima_period_0_error() {
        assert!(Trima::new("t", 0).is_err());
    }

    #[test]
    fn test_trima_unavailable_before_warmup() {
        // period=3 needs 2*3-1 = 5 bars
        let mut t = Trima::new("t3", 3).unwrap();
        for _ in 0..4 {
            assert_eq!(t.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(t.update_bar(&bar("100")).unwrap().is_scalar());
    }

    #[test]
    fn test_trima_constant_price_equals_price() {
        let mut t = Trima::new("t3", 3).unwrap();
        for _ in 0..10 {
            t.update_bar(&bar("100")).unwrap();
        }
        match t.update_bar(&bar("100")).unwrap() {
            SignalValue::Scalar(d) => assert_eq!(d, dec!(100)),
            _ => panic!("expected Scalar"),
        }
    }

    #[test]
    fn test_trima_reset() {
        let mut t = Trima::new("t3", 3).unwrap();
        for _ in 0..10 { t.update_bar(&bar("100")).unwrap(); }
        assert!(t.is_ready());
        t.reset();
        assert!(!t.is_ready());
    }
}
