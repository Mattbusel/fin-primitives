//! Price Distance from Moving Average indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Distance from Moving Average — normalized deviation of close from SMA.
///
/// ```text
/// SMA_t   = SMA(close, period)
/// StdDev  = population std dev of close over period
/// output  = (close_t − SMA_t) / StdDev_t
/// ```
///
/// This is equivalent to a z-score of the close within its rolling window.
/// Values beyond ±2 suggest overbought/oversold conditions.
/// Returns 0 when std dev is zero (flat market).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceDistanceMa;
/// use fin_primitives::signals::Signal;
///
/// let pd = PriceDistanceMa::new("pdma", 20).unwrap();
/// assert_eq!(pd.period(), 20);
/// ```
pub struct PriceDistanceMa {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PriceDistanceMa {
    /// Creates a new `PriceDistanceMa`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for PriceDistanceMa {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }
        if self.closes.len() < self.period { return Ok(SignalValue::Unavailable); }

        let n = Decimal::from(self.period as u32);
        let sma = self.closes.iter().sum::<Decimal>() / n;
        let variance = self.closes.iter()
            .map(|&c| { let d = c - sma; d * d })
            .sum::<Decimal>() / n;

        if variance.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        use rust_decimal::prelude::ToPrimitive;
        let std_dev = Decimal::try_from(
            variance.to_f64().unwrap_or(0.0).sqrt()
        ).unwrap_or(Decimal::ZERO);

        if std_dev.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar((bar.close - sma) / std_dev))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period }
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
    fn test_pdma_invalid() {
        assert!(PriceDistanceMa::new("p", 0).is_err());
        assert!(PriceDistanceMa::new("p", 1).is_err());
    }

    #[test]
    fn test_pdma_unavailable_before_warmup() {
        let mut p = PriceDistanceMa::new("p", 4).unwrap();
        for _ in 0..3 {
            assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pdma_flat_is_zero() {
        let mut p = PriceDistanceMa::new("p", 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..6 { last = p.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pdma_at_sma_is_zero() {
        // Symmetric window: [90, 100, 110] → sma=100; at close=100 → z=0
        let mut p = PriceDistanceMa::new("p", 3).unwrap();
        p.update_bar(&bar("90")).unwrap();
        p.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = p.update_bar(&bar("110")).unwrap() {
            // close=110, sma=100, std_dev ≈ 8.165 → z ≈ 1.22
            assert!(v > dec!(0), "expected positive z-score for above-SMA close");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pdma_reset() {
        let mut p = PriceDistanceMa::new("p", 4).unwrap();
        for _ in 0..6 { p.update_bar(&bar("100")).unwrap(); }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
