//! Close vs VWAP indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close vs VWAP — percentage deviation of the current close from the rolling
/// N-bar Volume-Weighted Average Price (VWAP):
///
/// ```text
/// VWAP  = Σ(close × volume) / Σ(volume)
/// signal = (close - VWAP) / VWAP × 100
/// ```
///
/// Positive values indicate price is above its volume-weighted baseline; negative
/// indicates below. This is useful for identifying intraday mean reversion setups.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseVsVwap;
/// use fin_primitives::signals::Signal;
///
/// let cvv = CloseVsVwap::new("cvv", 20).unwrap();
/// assert_eq!(cvv.period(), 20);
/// ```
pub struct CloseVsVwap {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    volumes: VecDeque<Decimal>,
}

impl CloseVsVwap {
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period),
            volumes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseVsVwap {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }
        self.volumes.push_back(bar.volume);
        if self.volumes.len() > self.period { self.volumes.pop_front(); }

        if self.closes.len() < self.period { return Ok(SignalValue::Unavailable); }

        let total_vol: Decimal = self.volumes.iter().sum();
        if total_vol.is_zero() { return Ok(SignalValue::Unavailable); }
        let vwap: Decimal = self.closes.iter().zip(self.volumes.iter())
            .map(|(c, v)| *c * *v)
            .sum::<Decimal>() / total_vol;
        if vwap.is_zero() { return Ok(SignalValue::Unavailable); }
        let pct = (bar.close - vwap) / vwap * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(pct))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.volumes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str, v: &str) -> OhlcvBar {
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let vq = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: cp, low: cp, close: cp,
            volume: vq,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cvv_invalid() { assert!(CloseVsVwap::new("c", 0).is_err()); }

    #[test]
    fn test_cvv_unavailable() {
        let mut cvv = CloseVsVwap::new("c", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(cvv.update_bar(&bar("100","1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_cvv_equal_weights_gives_zero() {
        // All bars same price and same volume → VWAP = price → dev = 0
        let mut cvv = CloseVsVwap::new("c", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 { last = cvv.update_bar(&bar("100","1000")).unwrap(); }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cvv_above_vwap_positive() {
        // Three bars: 90 (high vol), 90 (high vol), 110 (high vol)
        // VWAP = (90*1000 + 90*1000 + 110*1000)/3000 = 96.67
        // close=110, dev > 0
        let mut cvv = CloseVsVwap::new("c", 3).unwrap();
        cvv.update_bar(&bar("90","1000")).unwrap();
        cvv.update_bar(&bar("90","1000")).unwrap();
        let last = cvv.update_bar(&bar("110","1000")).unwrap();
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "close above VWAP should be positive: {}", v);
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_cvv_reset() {
        let mut cvv = CloseVsVwap::new("c", 3).unwrap();
        for _ in 0..3 { cvv.update_bar(&bar("100","1000")).unwrap(); }
        assert!(cvv.is_ready());
        cvv.reset();
        assert!(!cvv.is_ready());
    }
}
