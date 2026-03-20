//! Candle Range MA — SMA of bar range (high - low) over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Candle Range MA — simple moving average of bar range over `period` bars.
///
/// ```text
/// range[t]    = high[t] - low[t]
/// range_ma[t] = SMA(range, period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CandleRangeMa;
/// use fin_primitives::signals::Signal;
/// let crm = CandleRangeMa::new("crm", 10).unwrap();
/// assert_eq!(crm.period(), 10);
/// ```
pub struct CandleRangeMa {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CandleRangeMa {
    /// Constructs a new `CandleRangeMa`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for CandleRangeMa {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.window.push_back(range);
        self.sum += range;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let avg = self.sum / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(avg))
    }

    fn reset(&mut self) {
        self.window.clear();
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_crm_period_0_error() { assert!(CandleRangeMa::new("crm", 0).is_err()); }

    #[test]
    fn test_crm_unavailable_before_period() {
        let mut crm = CandleRangeMa::new("crm", 3).unwrap();
        assert_eq!(crm.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!crm.is_ready());
    }

    #[test]
    fn test_crm_constant_range() {
        let mut crm = CandleRangeMa::new("crm", 3).unwrap();
        crm.update_bar(&bar("110", "90")).unwrap();
        crm.update_bar(&bar("110", "90")).unwrap();
        let v = crm.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_crm_rolling_average() {
        let mut crm = CandleRangeMa::new("crm", 2).unwrap();
        crm.update_bar(&bar("110", "100")).unwrap(); // range=10
        let v = crm.update_bar(&bar("120", "100")).unwrap(); // range=20, avg=15
        assert_eq!(v, SignalValue::Scalar(dec!(15)));
    }

    #[test]
    fn test_crm_reset() {
        let mut crm = CandleRangeMa::new("crm", 2).unwrap();
        crm.update_bar(&bar("110", "90")).unwrap();
        crm.update_bar(&bar("110", "90")).unwrap();
        assert!(crm.is_ready());
        crm.reset();
        assert!(!crm.is_ready());
    }
}
