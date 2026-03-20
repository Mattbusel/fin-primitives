//! Rolling Mean Absolute Deviation — robust rolling volatility measure.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Mean Absolute Deviation (MAD) — `mean(|close - mean(close)|)` over N bars.
///
/// A robust alternative to standard deviation for measuring price dispersion:
/// - Less sensitive to outliers than standard deviation.
/// - Expressed in the same units as price.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingMAD;
/// use fin_primitives::signals::Signal;
/// let mad = RollingMAD::new("mad_14", 14).unwrap();
/// assert_eq!(mad.period(), 14);
/// ```
pub struct RollingMAD {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl RollingMAD {
    /// Constructs a new `RollingMAD`.
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
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RollingMAD {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.sum += bar.close;
        self.window.push_back(bar.close);
        if self.window.len() > self.period {
            let removed = self.window.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mean = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let mad_sum: Decimal = self.window.iter().map(|c| {
            let diff = *c - mean;
            if diff >= Decimal::ZERO { diff } else { -diff }
        }).sum();

        let mad = mad_sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(mad))
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
    fn test_mad_invalid_period() {
        assert!(RollingMAD::new("mad", 0).is_err());
    }

    #[test]
    fn test_mad_unavailable_before_period() {
        let mut s = RollingMAD::new("mad", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("102")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_mad_constant_prices_gives_zero() {
        let mut s = RollingMAD::new("mad", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100")).unwrap(); }
        let v = s.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_mad_symmetric_values() {
        let mut s = RollingMAD::new("mad", 4).unwrap();
        // [97, 99, 101, 103] → mean=100, deviations=[3,1,1,3] → MAD=2
        s.update_bar(&bar("97")).unwrap();
        s.update_bar(&bar("99")).unwrap();
        s.update_bar(&bar("101")).unwrap();
        let v = s.update_bar(&bar("103")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(2)).abs() < dec!(0.0001), "expected MAD=2, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_mad_non_negative() {
        let mut s = RollingMAD::new("mad", 5).unwrap();
        for p in &["100","102","99","103","101","104"] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(p)).unwrap() {
                assert!(v >= dec!(0), "MAD must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_mad_reset() {
        let mut s = RollingMAD::new("mad", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
