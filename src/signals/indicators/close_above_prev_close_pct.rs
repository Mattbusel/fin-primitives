//! Close Above Previous Close Percentage indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Above Previous Close Percentage — the fraction of bars over the last
/// `period` bars where `close > prev_close`.
///
/// Returns a value in [0, 1]:
/// - `1.0` → every bar closed higher than the previous  
/// - `0.0` → no bar closed higher  
/// - `0.5` → equal up/down split  
///
/// Returns [`SignalValue::Unavailable`] until `period` gaps have been observed.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAbovePrevClosePct;
/// use fin_primitives::signals::Signal;
///
/// let c = CloseAbovePrevClosePct::new("c", 10).unwrap();
/// assert_eq!(c.period(), 10);
/// ```
pub struct CloseAbovePrevClosePct {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    flags: VecDeque<bool>,
}

impl CloseAbovePrevClosePct {
    /// Constructs a new `CloseAbovePrevClosePct`.
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
            prev_close: None,
            flags: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseAbovePrevClosePct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.flags.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                let up = bar.close > pc;
                self.flags.push_back(up);
                if self.flags.len() > self.period {
                    self.flags.pop_front();
                }
                if self.flags.len() < self.period {
                    SignalValue::Unavailable
                } else {
                    let count = self.flags.iter().filter(|&&f| f).count();
                    let nd = Decimal::from(self.period as u32);
                    SignalValue::Scalar(Decimal::from(count as u32) / nd)
                }
            }
        };
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.flags.clear();
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
    fn test_capc_invalid_period() {
        assert!(CloseAbovePrevClosePct::new("c", 0).is_err());
    }

    #[test]
    fn test_capc_unavailable_before_warm_up() {
        let mut c = CloseAbovePrevClosePct::new("c", 3).unwrap();
        c.update_bar(&bar("100")).unwrap();
        assert_eq!(c.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_capc_all_up_gives_one() {
        let mut c = CloseAbovePrevClosePct::new("c", 3).unwrap();
        c.update_bar(&bar("100")).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 1u32..=3 {
            last = c.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_capc_all_down_gives_zero() {
        let mut c = CloseAbovePrevClosePct::new("c", 3).unwrap();
        c.update_bar(&bar("110")).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 1u32..=3 {
            last = c.update_bar(&bar(&(110 - i).to_string())).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_capc_reset() {
        let mut c = CloseAbovePrevClosePct::new("c", 3).unwrap();
        for i in 0u32..4 { c.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
