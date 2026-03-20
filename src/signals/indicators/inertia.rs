//! Inertia Indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Inertia Indicator — applies linear-regression smoothing to a running
/// momentum series to measure the "inertia" (persistence) of the trend.
///
/// ```text
/// momentum_t = close_t - close_{t-period}
/// inertia    = linear_regression_value(momentum_window, reg_period)
/// ```
///
/// A rising Inertia value signals an accelerating trend; falling signals deceleration.
/// Values above zero indicate persistent bullish momentum; below zero bearish.
///
/// Returns [`SignalValue::Unavailable`] until `period + reg_period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Inertia;
/// use fin_primitives::signals::Signal;
///
/// let ind = Inertia::new("inertia", 10, 14).unwrap();
/// assert_eq!(ind.period(), 14);
/// ```
pub struct Inertia {
    name: String,
    mom_period: usize,
    reg_period: usize,
    closes: VecDeque<Decimal>,
    mom_buf: VecDeque<Decimal>,
}

impl Inertia {
    /// Creates a new `Inertia`.
    ///
    /// - `mom_period`: lookback for momentum (`close - close[n]`).
    /// - `reg_period`: linear regression window applied to momentum values.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is zero, or if `reg_period < 2`.
    pub fn new(name: impl Into<String>, mom_period: usize, reg_period: usize) -> Result<Self, FinError> {
        if mom_period == 0 { return Err(FinError::InvalidPeriod(mom_period)); }
        if reg_period < 2  { return Err(FinError::InvalidPeriod(reg_period)); }
        Ok(Self {
            name: name.into(),
            mom_period,
            reg_period,
            closes: VecDeque::with_capacity(mom_period + 1),
            mom_buf: VecDeque::with_capacity(reg_period),
        })
    }

    fn linreg_value(buf: &VecDeque<Decimal>) -> Option<Decimal> {
        let n = buf.len();
        if n < 2 { return None; }
        let nf = n as f64;
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let ys: Vec<f64> = buf.iter().filter_map(|v| v.to_f64()).collect();
        if ys.len() != n { return None; }
        let sx: f64 = xs.iter().sum();
        let sy: f64 = ys.iter().sum();
        let sxy: f64 = xs.iter().zip(ys.iter()).map(|(x, y)| x * y).sum();
        let sxx: f64 = xs.iter().map(|x| x * x).sum();
        let denom = nf * sxx - sx * sx;
        if denom.abs() < f64::EPSILON { return None; }
        let b = (nf * sxy - sx * sy) / denom;
        let a = (sy - b * sx) / nf;
        // Value at last x (n-1)
        let last_x = (n - 1) as f64;
        Decimal::try_from(a + b * last_x).ok()
    }
}

impl Signal for Inertia {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.mom_period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() <= self.mom_period {
            return Ok(SignalValue::Unavailable);
        }

        let momentum = self.closes.back().copied().unwrap()
            - self.closes.front().copied().unwrap();

        self.mom_buf.push_back(momentum);
        if self.mom_buf.len() > self.reg_period {
            self.mom_buf.pop_front();
        }
        if self.mom_buf.len() < self.reg_period {
            return Ok(SignalValue::Unavailable);
        }

        match Self::linreg_value(&self.mom_buf) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool {
        self.mom_buf.len() >= self.reg_period
    }

    fn period(&self) -> usize {
        self.reg_period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.mom_buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};

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
    fn test_inertia_invalid() {
        assert!(Inertia::new("i", 0, 14).is_err());
        assert!(Inertia::new("i", 10, 1).is_err()); // reg_period < 2
    }

    #[test]
    fn test_inertia_unavailable_before_warmup() {
        let mut ind = Inertia::new("i", 3, 4).unwrap();
        // Needs mom_period+1 + (reg_period-1) = 4 + 3 = 7 bars before first scalar
        for _ in 0..6 {
            assert_eq!(ind.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_inertia_produces_scalar() {
        let mut ind = Inertia::new("i", 3, 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0..20usize {
            last = ind.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(matches!(last, SignalValue::Scalar(_)), "expected Scalar after warmup");
    }

    #[test]
    fn test_inertia_flat_near_zero() {
        let mut ind = Inertia::new("i", 3, 4).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = ind.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v.abs() < rust_decimal_macros::dec!(0.001), "flat price inertia should be ~0: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_inertia_reset() {
        let mut ind = Inertia::new("i", 3, 4).unwrap();
        for i in 0..20usize { ind.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        assert!(ind.is_ready());
        ind.reset();
        assert!(!ind.is_ready());
    }
}
