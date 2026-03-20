//! Donchian Channel Width indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Donchian Channel Width over `period` bars.
///
/// `Width = (highest_high - lowest_low) / midpoint * 100`
///
/// where `midpoint = (highest_high + lowest_low) / 2`.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
/// Returns zero when midpoint is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DonchianWidth;
/// use fin_primitives::signals::Signal;
///
/// let dw = DonchianWidth::new("dw20", 20).unwrap();
/// assert_eq!(dw.period(), 20);
/// ```
pub struct DonchianWidth {
    name: String,
    period: usize,
    history: VecDeque<BarInput>,
}

impl DonchianWidth {
    /// Constructs a new `DonchianWidth` with the given name and period.
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

impl Signal for DonchianWidth {
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
        let lowest  = self.history.iter().map(|b| b.low).fold(Decimal::MAX, Decimal::min);
        let midpoint = (highest + lowest) / Decimal::TWO;
        if midpoint.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        Ok(SignalValue::Scalar((highest - lowest) / midpoint * Decimal::ONE_HUNDRED))
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
    fn test_donchian_width_invalid_period() {
        assert!(DonchianWidth::new("dw", 0).is_err());
    }

    #[test]
    fn test_donchian_width_unavailable_before_period() {
        let mut dw = DonchianWidth::new("dw2", 2).unwrap();
        assert_eq!(dw.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_donchian_width_correct() {
        // period=2: highs=[110,120], lows=[90,80] → highest=120, lowest=80
        // midpoint=100, width=(40/100)*100=40
        let mut dw = DonchianWidth::new("dw2", 2).unwrap();
        dw.update_bar(&bar("110", "90")).unwrap();
        let v = dw.update_bar(&bar("120", "80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(40)));
    }

    #[test]
    fn test_donchian_width_constant_bars() {
        // high=110, low=90, mid=100, width=(20/100)*100=20
        let mut dw = DonchianWidth::new("dw3", 3).unwrap();
        dw.update_bar(&bar("110", "90")).unwrap();
        dw.update_bar(&bar("110", "90")).unwrap();
        let v = dw.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_donchian_width_reset() {
        let mut dw = DonchianWidth::new("dw2", 2).unwrap();
        dw.update_bar(&bar("110", "90")).unwrap();
        dw.update_bar(&bar("120", "80")).unwrap();
        assert!(dw.is_ready());
        dw.reset();
        assert!(!dw.is_ready());
    }
}
