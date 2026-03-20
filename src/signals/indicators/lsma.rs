//! Least Squares Moving Average (LSMA) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Least Squares Moving Average — the endpoint of a linear regression line over `period` bars.
///
/// Reduces lag compared to SMA by fitting a line through recent closes and projecting to the
/// current bar. The result is the value at position `period` on the best-fit line.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Lsma;
/// use fin_primitives::signals::Signal;
///
/// let mut lsma = Lsma::new("lsma25", 25).unwrap();
/// assert_eq!(lsma.period(), 25);
/// ```
pub struct Lsma {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl Lsma {
    /// Constructs a new `Lsma`.
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
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Lsma {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period;
        // x values: 1..=n, y values: closes (oldest first)
        #[allow(clippy::cast_possible_truncation)]
        let n_d = Decimal::from(n as u32);
        // sum_x = n(n+1)/2, sum_x2 = n(n+1)(2n+1)/6
        #[allow(clippy::cast_possible_truncation)]
        let sum_x = Decimal::from((n * (n + 1) / 2) as u64);
        #[allow(clippy::cast_possible_truncation)]
        let sum_x2 = Decimal::from((n * (n + 1) * (2 * n + 1) / 6) as u64);

        let mut sum_y = Decimal::ZERO;
        let mut sum_xy = Decimal::ZERO;
        for (i, &y) in self.closes.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let x = Decimal::from((i + 1) as u32);
            sum_y += y;
            sum_xy += x * y;
        }

        let denom = n_d * sum_x2 - sum_x * sum_x;
        if denom == Decimal::ZERO {
            return Ok(SignalValue::Scalar(sum_y / n_d));
        }

        let slope = (n_d * sum_xy - sum_x * sum_y) / denom;
        let intercept = (sum_y - slope * sum_x) / n_d;
        // endpoint: x = n
        #[allow(clippy::cast_possible_truncation)]
        let endpoint = slope * Decimal::from(n as u32) + intercept;
        Ok(SignalValue::Scalar(endpoint))
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
    fn test_lsma_period_0_error() {
        assert!(Lsma::new("l", 0).is_err());
    }

    #[test]
    fn test_lsma_unavailable_before_period() {
        let mut l = Lsma::new("l3", 3).unwrap();
        assert_eq!(l.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(l.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(l.update_bar(&bar("102")).unwrap().is_scalar());
    }

    #[test]
    fn test_lsma_constant_price_equals_price() {
        let mut l = Lsma::new("l3", 3).unwrap();
        for _ in 0..5 { l.update_bar(&bar("100")).unwrap(); }
        match l.update_bar(&bar("100")).unwrap() {
            SignalValue::Scalar(v) => assert_eq!(v, dec!(100)),
            _ => panic!("expected scalar"),
        }
    }

    #[test]
    fn test_lsma_linear_series_endpoint() {
        // closes: 1,2,3 → perfect line → endpoint at x=3 is 3
        let mut l = Lsma::new("l3", 3).unwrap();
        l.update_bar(&bar("1")).unwrap();
        l.update_bar(&bar("2")).unwrap();
        match l.update_bar(&bar("3")).unwrap() {
            SignalValue::Scalar(v) => assert_eq!(v, dec!(3)),
            _ => panic!("expected scalar"),
        }
    }

    #[test]
    fn test_lsma_reset() {
        let mut l = Lsma::new("l3", 3).unwrap();
        for _ in 0..5 { l.update_bar(&bar("100")).unwrap(); }
        assert!(l.is_ready());
        l.reset();
        assert!(!l.is_ready());
    }
}
