//! Signed Gap Sum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Signed Gap Sum — the rolling sum of overnight gaps (`open - prev_close`) over
/// the last `period` bars.
///
/// A positive value indicates a net upward gap bias; negative indicates a net
/// downward gap bias. This can reveal systematic opening auction pressure.
///
/// Returns [`SignalValue::Unavailable`] until 2 bars have been seen (a gap
/// requires a prior close).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SignedGapSum;
/// use fin_primitives::signals::Signal;
///
/// let sgs = SignedGapSum::new("sgs", 10).unwrap();
/// assert_eq!(sgs.period(), 10);
/// ```
pub struct SignedGapSum {
    name: String,
    period: usize,
    gaps: VecDeque<Decimal>,
    prev_close: Option<Decimal>,
}

impl SignedGapSum {
    /// Constructs a new `SignedGapSum`.
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
            gaps: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for SignedGapSum {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.gaps.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let gap = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(pc) => bar.open - pc,
        };
        self.prev_close = Some(bar.close);

        self.gaps.push_back(gap);
        if self.gaps.len() > self.period { self.gaps.pop_front(); }

        if self.gaps.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let sum: Decimal = self.gaps.iter().sum();
        Ok(SignalValue::Scalar(sum))
    }

    fn reset(&mut self) {
        self.gaps.clear();
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
    fn test_sgs_invalid_period() {
        assert!(SignedGapSum::new("s", 0).is_err());
    }

    #[test]
    fn test_sgs_unavailable_before_warm_up() {
        let mut sgs = SignedGapSum::new("s", 3).unwrap();
        // bar 1: no prev_close → Unavailable
        assert_eq!(sgs.update_bar(&bar("100", "100")).unwrap(), SignalValue::Unavailable);
        // bar 2: 1 gap, need 3
        assert_eq!(sgs.update_bar(&bar("101", "101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_sgs_positive_gap_bias() {
        let mut sgs = SignedGapSum::new("s", 3).unwrap();
        // Each open is 2 above prev close → gap = +2 each time
        sgs.update_bar(&bar("100", "100")).unwrap(); // seed
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = sgs.update_bar(&bar("102", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(6)));
    }

    #[test]
    fn test_sgs_zero_gap() {
        let mut sgs = SignedGapSum::new("s", 3).unwrap();
        sgs.update_bar(&bar("100", "100")).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = sgs.update_bar(&bar("100", "100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_sgs_reset() {
        let mut sgs = SignedGapSum::new("s", 2).unwrap();
        sgs.update_bar(&bar("100", "100")).unwrap();
        sgs.update_bar(&bar("102", "102")).unwrap();
        sgs.update_bar(&bar("104", "104")).unwrap();
        assert!(sgs.is_ready());
        sgs.reset();
        assert!(!sgs.is_ready());
    }
}
