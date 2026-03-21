//! Session Range Percentage indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Session Range Percentage.
///
/// Rolling mean of the session range as a percentage of the open price.
/// Expresses how wide the bar's range is relative to where the session started.
///
/// Per-bar formula: `range_pct = (high - low) / open * 100` (0 when open == 0)
///
/// Rolling: `mean(range_pct, period)`
///
/// - High value: wide sessions relative to the open.
/// - Low value: tight, calm sessions.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SessionRangePct;
/// use fin_primitives::signals::Signal;
/// let srp = SessionRangePct::new("srp_14", 14).unwrap();
/// assert_eq!(srp.period(), 14);
/// ```
pub struct SessionRangePct {
    name: String,
    period: usize,
    pcts: VecDeque<Decimal>,
}

impl SessionRangePct {
    /// Constructs a new `SessionRangePct`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, pcts: VecDeque::with_capacity(period) })
    }
}

impl Signal for SessionRangePct {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let pct = if bar.open.is_zero() {
            Decimal::ZERO
        } else {
            (bar.high - bar.low)
                .checked_div(bar.open)
                .ok_or(FinError::ArithmeticOverflow)?
                .checked_mul(Decimal::from(100u32))
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.pcts.push_back(pct);
        if self.pcts.len() > self.period {
            self.pcts.pop_front();
        }
        if self.pcts.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.pcts.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.pcts.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.pcts.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hi, low: lo, close: op,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(SessionRangePct::new("srp", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut srp = SessionRangePct::new("srp", 3).unwrap();
        assert_eq!(srp.update_bar(&bar("100", "105", "95")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_known_range_pct() {
        // open=100, high=110, low=90 → range=20, pct=20%
        let mut srp = SessionRangePct::new("srp", 3).unwrap();
        for _ in 0..3 {
            srp.update_bar(&bar("100", "110", "90")).unwrap();
        }
        let v = srp.update_bar(&bar("100", "110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_flat_session_zero_range() {
        let mut srp = SessionRangePct::new("srp", 3).unwrap();
        for _ in 0..3 {
            srp.update_bar(&bar("100", "100", "100")).unwrap();
        }
        let v = srp.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut srp = SessionRangePct::new("srp", 2).unwrap();
        srp.update_bar(&bar("100", "110", "90")).unwrap();
        srp.update_bar(&bar("100", "110", "90")).unwrap();
        assert!(srp.is_ready());
        srp.reset();
        assert!(!srp.is_ready());
    }
}
