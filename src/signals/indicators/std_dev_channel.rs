//! Standard Deviation Channel indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Standard Deviation Channel — measures how many standard deviations the current
/// close is from the rolling mean of closes.
///
/// ```text
/// z = (close - mean(close, period)) / stddev(close, period)
/// ```
///
/// Positive values mean the close is above the channel center; negative values below.
/// The classic ±1 and ±2 levels act as dynamic support/resistance.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// when standard deviation is zero (flat prices).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::StdDevChannel;
/// use fin_primitives::signals::Signal;
///
/// let s = StdDevChannel::new("sdc", 20).unwrap();
/// assert_eq!(s.period(), 20);
/// ```
pub struct StdDevChannel {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl StdDevChannel {
    /// Creates a new `StdDevChannel`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            history: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for StdDevChannel {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        self.history.push_back(bar.close);
        if self.history.len() > self.period {
            self.history.pop_front();
        }
        if self.history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let vals: Vec<f64> = self.history.iter()
            .filter_map(|c| c.to_f64())
            .collect();
        let mean = vals.iter().sum::<f64>() / n;
        let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let std = variance.sqrt();
        if std == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let close_f = bar.close.to_f64().unwrap_or(0.0);
        let z = (close_f - mean) / std;

        Ok(SignalValue::Scalar(
            Decimal::try_from(z).unwrap_or(Decimal::ZERO),
        ))
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
    fn test_sdc_invalid_period() {
        assert!(StdDevChannel::new("s", 0).is_err());
        assert!(StdDevChannel::new("s", 1).is_err());
    }

    #[test]
    fn test_sdc_unavailable_before_period() {
        let mut s = StdDevChannel::new("s", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_sdc_flat_unavailable() {
        // Flat prices → std = 0 → Unavailable
        let mut s = StdDevChannel::new("s", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100")).unwrap(); }
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_sdc_above_mean_positive() {
        let mut s = StdDevChannel::new("s", 3).unwrap();
        s.update_bar(&bar("98")).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("102")).unwrap(); // mean=100, last bar is at mean → z≈0
        if let SignalValue::Scalar(v) = s.update_bar(&bar("104")).unwrap() {
            // window [100,102,104], mean=102, last=104 → above mean → positive
            assert!(v > dec!(0), "above mean should be positive: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_sdc_below_mean_negative() {
        let mut s = StdDevChannel::new("s", 3).unwrap();
        s.update_bar(&bar("98")).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("102")).unwrap();
        if let SignalValue::Scalar(v) = s.update_bar(&bar("96")).unwrap() {
            // window [100,102,96], mean=99.33, last=96 → below mean → negative
            assert!(v < dec!(0), "below mean should be negative: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_sdc_reset() {
        let mut s = StdDevChannel::new("s", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
