//! Normalized Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Normalized Momentum — Z-score of the close relative to its own rolling
/// distribution: `(close - SMA(n)) / StdDev(close, n)`.
///
/// Unlike raw momentum, this is scale-independent and measures how many
/// standard deviations the current close is above or below its rolling mean.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// if std dev is zero (constant price).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NormalizedMomentum;
/// use fin_primitives::signals::Signal;
///
/// let nm = NormalizedMomentum::new("nm", 20).unwrap();
/// assert_eq!(nm.period(), 20);
/// ```
pub struct NormalizedMomentum {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl NormalizedMomentum {
    /// Constructs a new `NormalizedMomentum`.
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
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for NormalizedMomentum {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let vals: Vec<f64> = self.closes.iter().filter_map(|c| c.to_f64()).collect();
        if vals.len() != self.period {
            return Ok(SignalValue::Unavailable);
        }

        let nf = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / nf;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / nf;
        let std_dev = var.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let close_f = match bar.close.to_f64() {
            Some(f) => f,
            None => return Ok(SignalValue::Unavailable),
        };

        match Decimal::from_f64((close_f - mean) / std_dev) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) {
        self.closes.clear();
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
    fn test_nm_invalid_period() {
        assert!(NormalizedMomentum::new("nm", 0).is_err());
        assert!(NormalizedMomentum::new("nm", 1).is_err());
    }

    #[test]
    fn test_nm_unavailable_before_warm_up() {
        let mut nm = NormalizedMomentum::new("nm", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(nm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_nm_constant_price_unavailable() {
        let mut nm = NormalizedMomentum::new("nm", 3).unwrap();
        for _ in 0..3 {
            let result = nm.update_bar(&bar("100")).unwrap();
            // std dev = 0 → Unavailable
            assert_eq!(result, SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_nm_above_mean_positive() {
        // prices 90, 100, 110 → mean=100, close=110 → positive z-score
        let mut nm = NormalizedMomentum::new("nm", 3).unwrap();
        nm.update_bar(&bar("90")).unwrap();
        nm.update_bar(&bar("100")).unwrap();
        let result = nm.update_bar(&bar("110")).unwrap();
        if let SignalValue::Scalar(v) = result {
            assert!(v > dec!(0), "close above mean should give positive z-score: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_nm_below_mean_negative() {
        let mut nm = NormalizedMomentum::new("nm", 3).unwrap();
        nm.update_bar(&bar("110")).unwrap();
        nm.update_bar(&bar("100")).unwrap();
        let result = nm.update_bar(&bar("90")).unwrap();
        if let SignalValue::Scalar(v) = result {
            assert!(v < dec!(0), "close below mean should give negative z-score: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_nm_reset() {
        let mut nm = NormalizedMomentum::new("nm", 3).unwrap();
        for p in ["90", "100", "110"] { nm.update_bar(&bar(p)).unwrap(); }
        assert!(nm.is_ready());
        nm.reset();
        assert!(!nm.is_ready());
    }
}
