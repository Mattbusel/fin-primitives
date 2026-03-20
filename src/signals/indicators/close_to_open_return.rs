//! Close-to-Open Return indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close-to-Open Return — rolling average of overnight returns:
/// `(open - prev_close) / prev_close * 100`.
///
/// Positive values indicate a systematic upward drift during non-trading hours;
/// negative values indicate a downward overnight bias.
///
/// Returns [`SignalValue::Unavailable`] until `period` gaps have been seen,
/// or if any reference close is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseToOpenReturn;
/// use fin_primitives::signals::Signal;
///
/// let ctor = CloseToOpenReturn::new("ctor", 10).unwrap();
/// assert_eq!(ctor.period(), 10);
/// ```
pub struct CloseToOpenReturn {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseToOpenReturn {
    /// Constructs a new `CloseToOpenReturn`.
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
            returns: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for CloseToOpenReturn {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                if pc.is_zero() {
                    SignalValue::Unavailable
                } else {
                    let ret = (bar.open - pc) / pc * Decimal::ONE_HUNDRED;
                    self.returns.push_back(ret);
                    self.sum += ret;
                    if self.returns.len() > self.period {
                        self.sum -= self.returns.pop_front().unwrap();
                    }
                    if self.returns.len() < self.period {
                        SignalValue::Unavailable
                    } else {
                        let nd = Decimal::from(self.period as u32);
                        SignalValue::Scalar(self.sum / nd)
                    }
                }
            }
        };
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.returns.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp, low: op, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ctor_invalid_period() {
        assert!(CloseToOpenReturn::new("ctor", 0).is_err());
    }

    #[test]
    fn test_ctor_unavailable_before_warm_up() {
        let mut ctor = CloseToOpenReturn::new("ctor", 3).unwrap();
        assert_eq!(ctor.update_bar(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(ctor.update_bar(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ctor_positive_overnight_gap() {
        // Each bar opens 1% above prev close
        let mut ctor = CloseToOpenReturn::new("ctor", 3).unwrap();
        ctor.update_bar(&bar("100", "100")).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = ctor.update_bar(&bar("101", "100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "positive gap should give positive return: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ctor_zero_gap() {
        // Each bar opens exactly at prev close → return = 0
        let mut ctor = CloseToOpenReturn::new("ctor", 3).unwrap();
        ctor.update_bar(&bar("100", "100")).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = ctor.update_bar(&bar("100", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ctor_reset() {
        let mut ctor = CloseToOpenReturn::new("ctor", 3).unwrap();
        for _ in 0..4 { ctor.update_bar(&bar("100", "100")).unwrap(); }
        assert!(ctor.is_ready());
        ctor.reset();
        assert!(!ctor.is_ready());
    }
}
