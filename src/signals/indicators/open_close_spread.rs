//! Open-Close Spread indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Open-Close Spread — rolling average of `|open - prev_close| / prev_close * 100`,
/// measuring the typical overnight jump/gap magnitude as a percentage.
///
/// Unlike `SignedGapSum`, this captures the average absolute gap without direction,
/// useful for measuring the noise introduced by overnight pricing.
///
/// Returns [`SignalValue::Unavailable`] until `period` gaps have been observed.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenCloseSpread;
/// use fin_primitives::signals::Signal;
///
/// let ocs = OpenCloseSpread::new("ocs", 20).unwrap();
/// assert_eq!(ocs.period(), 20);
/// ```
pub struct OpenCloseSpread {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    gaps: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenCloseSpread {
    /// Constructs a new `OpenCloseSpread`.
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
            gaps: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for OpenCloseSpread {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.gaps.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                let gap = if pc.is_zero() {
                    Decimal::ZERO
                } else {
                    (bar.open - pc).abs() / pc * Decimal::ONE_HUNDRED
                };
                self.gaps.push_back(gap);
                self.sum += gap;
                if self.gaps.len() > self.period {
                    self.sum -= self.gaps.pop_front().unwrap();
                }
                if self.gaps.len() < self.period {
                    SignalValue::Unavailable
                } else {
                    let nd = Decimal::from(self.period as u32);
                    SignalValue::Scalar(self.sum / nd)
                }
            }
        };
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.gaps.clear();
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
    fn test_ocs_invalid_period() {
        assert!(OpenCloseSpread::new("ocs", 0).is_err());
    }

    #[test]
    fn test_ocs_unavailable_before_warm_up() {
        let mut ocs = OpenCloseSpread::new("ocs", 3).unwrap();
        ocs.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(ocs.update_bar(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ocs_no_gaps() {
        let mut ocs = OpenCloseSpread::new("ocs", 3).unwrap();
        ocs.update_bar(&bar("100", "100")).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = ocs.update_bar(&bar("100", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ocs_constant_gap() {
        // Each bar opens 1% above prev close
        let mut ocs = OpenCloseSpread::new("ocs", 3).unwrap();
        ocs.update_bar(&bar("100", "100")).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = ocs.update_bar(&bar("101", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ocs_reset() {
        let mut ocs = OpenCloseSpread::new("ocs", 3).unwrap();
        for _ in 0..4 { ocs.update_bar(&bar("100", "100")).unwrap(); }
        assert!(ocs.is_ready());
        ocs.reset();
        assert!(!ocs.is_ready());
    }
}
