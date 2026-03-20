//! Cumulative Volume indicator — rolling N-bar sum of volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Cumulative Volume — rolling sum of traded volume over `period` bars.
///
/// ```text
/// cum_vol[t] = volume[t] + volume[t-1] + ... + volume[t-period+1]
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CumulativeVolume;
/// use fin_primitives::signals::Signal;
/// let cv = CumulativeVolume::new("cv", 5).unwrap();
/// assert_eq!(cv.period(), 5);
/// ```
pub struct CumulativeVolume {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CumulativeVolume {
    /// Constructs a new `CumulativeVolume`.
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

impl Signal for CumulativeVolume {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        self.sum += bar.volume;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(self.sum))
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

    fn bar(vol: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
        let v = Quantity::new(vol.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p, volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cv_period_0_error() { assert!(CumulativeVolume::new("cv", 0).is_err()); }

    #[test]
    fn test_cv_unavailable_before_period() {
        let mut cv = CumulativeVolume::new("cv", 3).unwrap();
        assert_eq!(cv.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!cv.is_ready());
    }

    #[test]
    fn test_cv_sum_correct() {
        let mut cv = CumulativeVolume::new("cv", 3).unwrap();
        cv.update_bar(&bar("100")).unwrap();
        cv.update_bar(&bar("200")).unwrap();
        let v = cv.update_bar(&bar("300")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(600)));
    }

    #[test]
    fn test_cv_rolls_out_old() {
        let mut cv = CumulativeVolume::new("cv", 3).unwrap();
        cv.update_bar(&bar("100")).unwrap();
        cv.update_bar(&bar("200")).unwrap();
        cv.update_bar(&bar("300")).unwrap(); // sum=600
        let v = cv.update_bar(&bar("400")).unwrap(); // 100 leaves: 200+300+400=900
        assert_eq!(v, SignalValue::Scalar(dec!(900)));
    }

    #[test]
    fn test_cv_reset() {
        let mut cv = CumulativeVolume::new("cv", 2).unwrap();
        cv.update_bar(&bar("100")).unwrap();
        cv.update_bar(&bar("200")).unwrap();
        assert!(cv.is_ready());
        cv.reset();
        assert!(!cv.is_ready());
        assert_eq!(cv.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
