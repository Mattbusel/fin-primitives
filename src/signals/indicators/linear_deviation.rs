//! Linear Deviation indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Linear Deviation — the percentage distance between the current close and the
/// least-squares linear regression value for that bar over the last `period` bars.
///
/// ```text
/// linear_dev = (close - linreg_value) / close × 100
/// ```
///
/// Positive values mean price is above the regression line (overbought pressure);
/// negative values mean price is below (oversold pressure).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or close is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LinearDeviation;
/// use fin_primitives::signals::Signal;
///
/// let ld = LinearDeviation::new("ld", 14).unwrap();
/// assert_eq!(ld.period(), 14);
/// ```
pub struct LinearDeviation {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl LinearDeviation {
    /// Creates a new `LinearDeviation`.
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

impl Signal for LinearDeviation {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() > self.period {
            self.history.pop_front();
        }
        if self.history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as i64;
        // x values: 0, 1, ..., n-1
        // Σx = n*(n-1)/2,  Σx² = n*(n-1)*(2n-1)/6
        let sum_x = Decimal::from(n * (n - 1) / 2);
        let sum_x2 = Decimal::from(n * (n - 1) * (2 * n - 1) / 6);
        let sum_y: Decimal = self.history.iter().sum();
        let sum_xy: Decimal = self.history.iter().enumerate()
            .map(|(i, &y)| Decimal::from(i as i64) * y)
            .sum();

        let n_dec = Decimal::from(n);
        let denom = n_dec * sum_x2 - sum_x * sum_x;
        if denom.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let slope = (n_dec * sum_xy - sum_x * sum_y)
            .checked_div(denom)
            .ok_or(FinError::ArithmeticOverflow)?;
        let intercept = (sum_y - slope * sum_x)
            .checked_div(n_dec)
            .ok_or(FinError::ArithmeticOverflow)?;

        // linreg value at x = n-1 (the last bar)
        let x_last = Decimal::from(n - 1);
        let linreg_val = slope * x_last + intercept;

        let close = bar.close;
        if close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let dev = (close - linreg_val)
            .checked_div(close)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::from(100u32);

        Ok(SignalValue::Scalar(dev))
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period
    }

    fn period(&self) -> usize { self.period }

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
    fn test_ld_invalid_period() {
        assert!(LinearDeviation::new("l", 0).is_err());
        assert!(LinearDeviation::new("l", 1).is_err());
    }

    #[test]
    fn test_ld_unavailable_early() {
        let mut ld = LinearDeviation::new("l", 3).unwrap();
        assert_eq!(ld.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(ld.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ld_perfectly_on_line_is_zero() {
        // Perfect linear: 100, 101, 102 — last bar IS the regression value
        let mut ld = LinearDeviation::new("l", 3).unwrap();
        ld.update_bar(&bar("100")).unwrap();
        ld.update_bar(&bar("101")).unwrap();
        if let SignalValue::Scalar(v) = ld.update_bar(&bar("102")).unwrap() {
            assert!(v.abs() < dec!(0.001), "on-line deviation should be ~0: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ld_above_line_positive() {
        // 100, 100, 110 — last bar spikes above trend
        let mut ld = LinearDeviation::new("l", 3).unwrap();
        ld.update_bar(&bar("100")).unwrap();
        ld.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = ld.update_bar(&bar("110")).unwrap() {
            assert!(v > dec!(0), "above line → positive deviation: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ld_below_line_negative() {
        // 100, 100, 90 — last bar drops below trend
        let mut ld = LinearDeviation::new("l", 3).unwrap();
        ld.update_bar(&bar("100")).unwrap();
        ld.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(v) = ld.update_bar(&bar("90")).unwrap() {
            assert!(v < dec!(0), "below line → negative deviation: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ld_reset() {
        let mut ld = LinearDeviation::new("l", 3).unwrap();
        for p in &["100", "101", "102"] { ld.update_bar(&bar(p)).unwrap(); }
        assert!(ld.is_ready());
        ld.reset();
        assert!(!ld.is_ready());
    }
}
