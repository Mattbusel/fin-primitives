//! Price Change MAD indicator.
//!
//! Rolling mean absolute deviation of raw close-to-close price changes.
//! Unlike historical volatility (std dev of returns), this is in price units
//! and uses MAD — more robust to outliers.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Change MAD — rolling mean of `|close[i] - close[i-1]|`.
///
/// ```text
/// change[i] = |close[i] - close[i-1]|
/// MAD[t]    = mean(change[t-period+1 .. t])
/// ```
///
/// This measures the typical absolute price movement per bar in price units.
/// Useful for:
/// - Position sizing in price-delta terms (vs vol-based sizing).
/// - Detecting periods of low-activity compression.
/// - Comparing liquidity across instruments with similar prices.
///
/// Returns [`SignalValue::Unavailable`] until `period` changes are collected
/// (`period + 1` bars).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceChangeMad;
/// use fin_primitives::signals::Signal;
/// let pcm = PriceChangeMad::new("pcm_14", 14).unwrap();
/// assert_eq!(pcm.period(), 14);
/// ```
pub struct PriceChangeMad {
    name: String,
    period: usize,
    changes: VecDeque<Decimal>,
    sum: Decimal,
    prev_close: Option<Decimal>,
}

impl PriceChangeMad {
    /// Constructs a new `PriceChangeMad`.
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
            changes: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
            prev_close: None,
        })
    }
}

impl Signal for PriceChangeMad {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.changes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        if let Some(prev) = self.prev_close {
            let change = (close - prev).abs();
            self.sum += change;
            self.changes.push_back(change);
            if self.changes.len() > self.period {
                let removed = self.changes.pop_front().unwrap();
                self.sum -= removed;
            }
        }
        self.prev_close = Some(close);

        if self.changes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mean = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(mean))
    }

    fn reset(&mut self) {
        self.changes.clear();
        self.sum = Decimal::ZERO;
        self.prev_close = None;
    }
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
    fn test_pcm_invalid_period() {
        assert!(PriceChangeMad::new("pcm", 0).is_err());
    }

    #[test]
    fn test_pcm_unavailable_during_warmup() {
        let mut pcm = PriceChangeMad::new("pcm", 3).unwrap();
        assert_eq!(pcm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(pcm.update_bar(&bar("102")).unwrap(), SignalValue::Unavailable);
        assert_eq!(pcm.update_bar(&bar("104")).unwrap(), SignalValue::Unavailable);
        assert!(!pcm.is_ready());
    }

    #[test]
    fn test_pcm_flat_prices_zero() {
        let mut pcm = PriceChangeMad::new("pcm", 3).unwrap();
        for _ in 0..4 { pcm.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = pcm.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pcm_constant_change() {
        // Price increases by 2 each bar: changes = [2, 2, 2] → MAD = 2
        let mut pcm = PriceChangeMad::new("pcm", 3).unwrap();
        for i in 0..=4u32 {
            pcm.update_bar(&bar(&(100 + i * 2).to_string())).unwrap();
        }
        if let SignalValue::Scalar(v) = pcm.update_bar(&bar("110")).unwrap() {
            assert_eq!(v, dec!(2));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pcm_mixed_moves() {
        // changes: |102-100|=2, |99-102|=3, |104-99|=5 → mean = 10/3
        let mut pcm = PriceChangeMad::new("pcm", 3).unwrap();
        pcm.update_bar(&bar("100")).unwrap();
        pcm.update_bar(&bar("102")).unwrap();
        pcm.update_bar(&bar("99")).unwrap();
        if let SignalValue::Scalar(v) = pcm.update_bar(&bar("104")).unwrap() {
            // mean = (2+3+5)/3 = 10/3 ≈ 3.333...
            let expected = dec!(10) / dec!(3);
            assert!((v - expected).abs() < dec!(0.0001), "expected ~{expected}, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pcm_sliding_window() {
        // After enough bars, old changes drop off
        let mut pcm = PriceChangeMad::new("pcm", 2).unwrap();
        pcm.update_bar(&bar("100")).unwrap(); // no change
        pcm.update_bar(&bar("110")).unwrap(); // change=10
        pcm.update_bar(&bar("112")).unwrap(); // change=2; window=[10,2] mean=6
        if let SignalValue::Scalar(v) = pcm.update_bar(&bar("114")).unwrap() {
            // window=[2,2], mean=2
            assert_eq!(v, dec!(2));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pcm_reset() {
        let mut pcm = PriceChangeMad::new("pcm", 2).unwrap();
        pcm.update_bar(&bar("100")).unwrap();
        pcm.update_bar(&bar("102")).unwrap();
        pcm.update_bar(&bar("104")).unwrap();
        assert!(pcm.is_ready());
        pcm.reset();
        assert!(!pcm.is_ready());
        assert_eq!(pcm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
