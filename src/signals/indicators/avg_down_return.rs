//! Average Down Return indicator.
//!
//! Rolling mean of close-to-close returns on bars where the close was below
//! the prior close. Returns a negative value representing the typical loss.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Average Down Return — rolling mean of returns on down bars only.
///
/// A bar is a "down bar" when `close[i] < close[i-1]`. Its return is:
/// ```text
/// ret[i] = (close[i] - close[i-1]) / close[i-1] × 100   (negative value)
/// ```
///
/// Only down-bar returns are averaged. The result is negative (or zero),
/// representing the typical magnitude of the loss per losing bar.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen
/// OR when no down bars exist in the current window.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AvgDownReturn;
/// use fin_primitives::signals::Signal;
/// let adr = AvgDownReturn::new("adr_20", 20).unwrap();
/// assert_eq!(adr.period(), 20);
/// ```
pub struct AvgDownReturn {
    name: String,
    period: usize,
    window: VecDeque<(f64, bool)>,
    prev_close: Option<f64>,
}

impl AvgDownReturn {
    /// Constructs a new `AvgDownReturn`.
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
            window: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for AvgDownReturn {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let c = bar.close.to_f64().unwrap_or(0.0);
        if let Some(pc) = self.prev_close {
            if pc > 0.0 {
                let ret = (c - pc) / pc * 100.0;
                let is_down = c < pc;
                self.window.push_back((ret, is_down));
                if self.window.len() > self.period {
                    self.window.pop_front();
                }
            }
        }
        self.prev_close = Some(c);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let down_rets: Vec<f64> = self.window.iter()
            .filter(|(_, is_down)| *is_down)
            .map(|(r, _)| *r)
            .collect();

        if down_rets.is_empty() {
            return Ok(SignalValue::Unavailable);
        }

        let avg = down_rets.iter().sum::<f64>() / down_rets.len() as f64;
        Decimal::try_from(avg)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn reset(&mut self) {
        self.window.clear();
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
    fn test_adr_invalid_period() {
        assert!(AvgDownReturn::new("adr", 0).is_err());
    }

    #[test]
    fn test_adr_unavailable_during_warmup() {
        let mut adr = AvgDownReturn::new("adr", 3).unwrap();
        adr.update_bar(&bar("100")).unwrap();
        adr.update_bar(&bar("98")).unwrap();
        adr.update_bar(&bar("96")).unwrap();
        assert!(!adr.is_ready());
    }

    #[test]
    fn test_adr_no_down_bars_unavailable() {
        let mut adr = AvgDownReturn::new("adr", 3).unwrap();
        adr.update_bar(&bar("100")).unwrap();
        adr.update_bar(&bar("102")).unwrap();
        adr.update_bar(&bar("104")).unwrap();
        assert_eq!(adr.update_bar(&bar("106")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_adr_all_down_bars_negative() {
        let mut adr = AvgDownReturn::new("adr", 3).unwrap();
        adr.update_bar(&bar("110")).unwrap();
        adr.update_bar(&bar("108")).unwrap();
        adr.update_bar(&bar("106")).unwrap();
        if let SignalValue::Scalar(v) = adr.update_bar(&bar("104")).unwrap() {
            assert!(v < dec!(0), "all down bars → negative avg down return: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_adr_reset() {
        let mut adr = AvgDownReturn::new("adr", 2).unwrap();
        adr.update_bar(&bar("110")).unwrap();
        adr.update_bar(&bar("108")).unwrap();
        adr.update_bar(&bar("106")).unwrap();
        assert!(adr.is_ready());
        adr.reset();
        assert!(!adr.is_ready());
    }
}
