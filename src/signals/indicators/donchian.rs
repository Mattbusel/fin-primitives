//! Donchian Midpoint indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Donchian Channel Midpoint over `period` bars.
///
/// `Midpoint = (highest_high(period) + lowest_low(period)) / 2`
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DonchianMidpoint;
/// use fin_primitives::signals::Signal;
///
/// let mut dc = DonchianMidpoint::new("dc20", 20).unwrap();
/// ```
pub struct DonchianMidpoint {
    name: String,
    period: usize,
    history: VecDeque<BarInput>,
}

impl DonchianMidpoint {
    /// Constructs a new `DonchianMidpoint` with the given name and period.
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
            history: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for DonchianMidpoint {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(*bar);
        if self.history.len() > self.period {
            self.history.pop_front();
        }
        if self.history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let highest = self.history.iter().map(|b| b.high).fold(Decimal::MIN, Decimal::max);
        let lowest = self.history.iter().map(|b| b.low).fold(Decimal::MAX, Decimal::min);
        Ok(SignalValue::Scalar((highest + lowest) / Decimal::TWO))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let h_p = Price::new(h.parse().unwrap()).unwrap();
        let l_p = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l_p, high: h_p, low: l_p, close: h_p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_donchian_period_0_error() {
        assert!(DonchianMidpoint::new("dc", 0).is_err());
    }

    #[test]
    fn test_donchian_unavailable_before_period() {
        let mut dc = DonchianMidpoint::new("dc2", 2).unwrap();
        assert_eq!(dc.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(dc.update_bar(&bar("120", "80")).unwrap().is_scalar());
    }

    #[test]
    fn test_donchian_midpoint_correct() {
        // period=2, bar1: h=110,l=90; bar2: h=120,l=80
        // highest_high=120, lowest_low=80 → mid = (120+80)/2 = 100
        let mut dc = DonchianMidpoint::new("dc2", 2).unwrap();
        dc.update_bar(&bar("110", "90")).unwrap();
        let v = dc.update_bar(&bar("120", "80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_donchian_constant_bars() {
        // high=110, low=90 for all → mid = (110+90)/2 = 100
        let mut dc = DonchianMidpoint::new("dc3", 3).unwrap();
        dc.update_bar(&bar("110", "90")).unwrap();
        dc.update_bar(&bar("110", "90")).unwrap();
        let v = dc.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_donchian_reset() {
        let mut dc = DonchianMidpoint::new("dc2", 2).unwrap();
        dc.update_bar(&bar("110", "90")).unwrap();
        dc.update_bar(&bar("120", "80")).unwrap();
        assert!(dc.is_ready());
        dc.reset();
        assert!(!dc.is_ready());
    }
}
