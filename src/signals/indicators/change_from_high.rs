//! Change From High indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Change From High — measures the current close as a percentage drawdown from
/// the highest close in the last `n` bars.
///
/// ```text
/// period_high = max(close[t-n+1 .. t])
/// cfh         = (close_t - period_high) / period_high × 100
/// ```
///
/// Values are always ≤ 0. Zero means the current close is at the period high.
/// A value of -10 means the close is 10% below the period high.
///
/// Returns [`SignalValue::Unavailable`] until `n` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ChangeFromHigh;
/// use fin_primitives::signals::Signal;
///
/// let cfh = ChangeFromHigh::new("cfh", 20).unwrap();
/// assert_eq!(cfh.period(), 20);
/// ```
pub struct ChangeFromHigh {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl ChangeFromHigh {
    /// Creates a new `ChangeFromHigh`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for ChangeFromHigh {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let high = self.closes.iter().cloned().fold(Decimal::MIN, Decimal::max);
        if high.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let cfh = (bar.close - high) / high * Decimal::from(100u32);
        Ok(SignalValue::Scalar(cfh))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
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
    fn test_cfh_invalid() {
        assert!(ChangeFromHigh::new("c", 0).is_err());
    }

    #[test]
    fn test_cfh_unavailable_before_period() {
        let mut cfh = ChangeFromHigh::new("c", 3).unwrap();
        assert_eq!(cfh.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(cfh.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cfh_at_high_is_zero() {
        let mut cfh = ChangeFromHigh::new("c", 3).unwrap();
        cfh.update_bar(&bar("100")).unwrap();
        cfh.update_bar(&bar("90")).unwrap();
        if let SignalValue::Scalar(v) = cfh.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0), "at period high → CFH = 0");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_cfh_below_high_negative() {
        let mut cfh = ChangeFromHigh::new("c", 3).unwrap();
        cfh.update_bar(&bar("100")).unwrap();
        cfh.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = cfh.update_bar(&bar("90")).unwrap() {
            assert!(v < dec!(0), "below period high → CFH < 0: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_cfh_reset() {
        let mut cfh = ChangeFromHigh::new("c", 3).unwrap();
        for _ in 0..3 { cfh.update_bar(&bar("100")).unwrap(); }
        assert!(cfh.is_ready());
        cfh.reset();
        assert!(!cfh.is_ready());
    }
}
