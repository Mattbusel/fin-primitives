//! Williams %R indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Williams %R oscillator over `period` bars.
///
/// `%R = (highest_high - close) / (highest_high - lowest_low) * -100`
///
/// Values range from -100 (oversold) to 0 (overbought). Returns
/// [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WilliamsR;
/// use fin_primitives::signals::Signal;
///
/// let mut wr = WilliamsR::new("wr14", 14).unwrap();
/// ```
pub struct WilliamsR {
    name: String,
    period: usize,
    history: VecDeque<BarInput>,
}

impl WilliamsR {
    /// Constructs a new `WilliamsR` with the given name and period.
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

impl Signal for WilliamsR {
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
        let range = highest - lowest;
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::from(-100i32)));
        }
        let wr = (highest - bar.close) / range * Decimal::from(-100i32);
        Ok(SignalValue::Scalar(wr))
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

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_williams_r_period_0_error() {
        assert!(WilliamsR::new("wr", 0).is_err());
    }

    #[test]
    fn test_williams_r_unavailable_until_period() {
        let mut wr = WilliamsR::new("wr3", 3).unwrap();
        assert_eq!(wr.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(wr.update_bar(&bar("105", "115", "95", "110")).unwrap(), SignalValue::Unavailable);
        assert!(wr.update_bar(&bar("110", "120", "100", "115")).unwrap().is_scalar());
    }

    #[test]
    fn test_williams_r_at_high_is_zero() {
        // When close == highest_high, %R = 0
        let mut wr = WilliamsR::new("wr1", 1).unwrap();
        let v = wr.update_bar(&bar("100", "120", "80", "120")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_williams_r_at_low_is_minus_100() {
        // When close == lowest_low, %R = -100
        let mut wr = WilliamsR::new("wr1", 1).unwrap();
        let v = wr.update_bar(&bar("100", "120", "80", "80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-100)));
    }

    #[test]
    fn test_williams_r_midpoint_is_minus_50() {
        // close at midpoint of range → %R = -50
        let mut wr = WilliamsR::new("wr1", 1).unwrap();
        let v = wr.update_bar(&bar("100", "120", "80", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-50)));
    }

    #[test]
    fn test_williams_r_reset_clears_state() {
        let mut wr = WilliamsR::new("wr2", 2).unwrap();
        wr.update_bar(&bar("100", "110", "90", "105")).unwrap();
        wr.update_bar(&bar("105", "115", "95", "110")).unwrap();
        assert!(wr.is_ready());
        wr.reset();
        assert!(!wr.is_ready());
    }
}
