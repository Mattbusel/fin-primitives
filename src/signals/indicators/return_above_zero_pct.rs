//! Return Above Zero Percentage indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Return Above Zero Percentage — the fraction of close-to-close returns over
/// `period` bars that are strictly positive.
///
/// Equivalent to a "batting average" for the period. A value > 0.5 indicates
/// more up days than down days.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ReturnAboveZeroPct;
/// use fin_primitives::signals::Signal;
///
/// let r = ReturnAboveZeroPct::new("r", 20).unwrap();
/// assert_eq!(r.period(), 20);
/// ```
pub struct ReturnAboveZeroPct {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    flags: VecDeque<bool>,
}

impl ReturnAboveZeroPct {
    /// Constructs a new `ReturnAboveZeroPct`.
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

impl Signal for ReturnAboveZeroPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.flags.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                let positive = bar.close > pc;
                self.flags.push_back(positive);
                if self.flags.len() > self.period { self.flags.pop_front(); }
                if self.flags.len() < self.period {
                    SignalValue::Unavailable
                } else {
                    let count = self.flags.iter().filter(|&&f| f).count();
                    SignalValue::Scalar(Decimal::from(count as u32) / Decimal::from(self.period as u32))
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
    fn test_razp_invalid_period() {
        assert!(ReturnAboveZeroPct::new("r", 0).is_err());
    }

    #[test]
    fn test_razp_unavailable_before_warm_up() {
        let mut r = ReturnAboveZeroPct::new("r", 3).unwrap();
        r.update_bar(&bar("100")).unwrap();
        assert_eq!(r.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_razp_all_up_gives_one() {
        let mut r = ReturnAboveZeroPct::new("r", 3).unwrap();
        r.update_bar(&bar("100")).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 1u32..=3 {
            last = r.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_razp_all_down_gives_zero() {
        let mut r = ReturnAboveZeroPct::new("r", 3).unwrap();
        r.update_bar(&bar("110")).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 1u32..=3 {
            last = r.update_bar(&bar(&(110 - i).to_string())).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_razp_reset() {
        let mut r = ReturnAboveZeroPct::new("r", 3).unwrap();
        for i in 0u32..4 { r.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
