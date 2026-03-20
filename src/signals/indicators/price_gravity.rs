//! Price Gravity indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Gravity — measures mean-reversion strength as the z-score of the current
/// close relative to its rolling mean and standard deviation:
///
/// ```text
/// gravity = (mean(close, n) - close) / std_dev(close, n)
/// ```
///
/// Note the direction is inverted vs normal z-score: positive gravity means the
/// price is below its mean (pull toward mean is upward), negative means overextended upward.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or std dev is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceGravity;
/// use fin_primitives::signals::Signal;
///
/// let pg = PriceGravity::new("pg", 20).unwrap();
/// assert_eq!(pg.period(), 20);
/// ```
pub struct PriceGravity {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PriceGravity {
    /// Constructs a new `PriceGravity`.
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

impl Signal for PriceGravity {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }

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

        // gravity = (mean - close) / std_dev (inverted: positive = price below mean)
        match Decimal::from_f64((mean - close_f) / std_dev) {
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
    fn test_pg_invalid_period() {
        assert!(PriceGravity::new("pg", 0).is_err());
        assert!(PriceGravity::new("pg", 1).is_err());
    }

    #[test]
    fn test_pg_unavailable_before_warm_up() {
        let mut pg = PriceGravity::new("pg", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(pg.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pg_constant_price_unavailable() {
        // std dev = 0 → Unavailable
        let mut pg = PriceGravity::new("pg", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(pg.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pg_below_mean_positive() {
        // prices 110, 100, 90 → mean=100, close=90 → gravity = (100-90)/std_dev > 0
        let mut pg = PriceGravity::new("pg", 3).unwrap();
        pg.update_bar(&bar("110")).unwrap();
        pg.update_bar(&bar("100")).unwrap();
        let result = pg.update_bar(&bar("90")).unwrap();
        if let SignalValue::Scalar(v) = result {
            assert!(v > dec!(0), "close below mean should give positive gravity: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pg_above_mean_negative() {
        let mut pg = PriceGravity::new("pg", 3).unwrap();
        pg.update_bar(&bar("90")).unwrap();
        pg.update_bar(&bar("100")).unwrap();
        let result = pg.update_bar(&bar("110")).unwrap();
        if let SignalValue::Scalar(v) = result {
            assert!(v < dec!(0), "close above mean should give negative gravity: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pg_reset() {
        let mut pg = PriceGravity::new("pg", 3).unwrap();
        for p in ["90", "100", "110"] { pg.update_bar(&bar(p)).unwrap(); }
        assert!(pg.is_ready());
        pg.reset();
        assert!(!pg.is_ready());
    }
}
