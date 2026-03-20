//! Rolling standard deviation indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling population standard deviation of close prices over `period` bars.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
/// Returns `Scalar(0)` when all closes in the window are identical.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::StdDev;
/// use fin_primitives::signals::Signal;
///
/// let mut sd = StdDev::new("sd5", 5).unwrap();
/// ```
pub struct StdDev {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl StdDev {
    /// Constructs a new `StdDev` with the given name and period.
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
            history: VecDeque::with_capacity(period),
        })
    }
}

fn decimal_sqrt(n: Decimal) -> Decimal {
    if n.is_zero() {
        return Decimal::ZERO;
    }
    let mut x = n;
    for _ in 0..20 {
        let next = (x + n / x) / Decimal::TWO;
        let diff = if next > x { next - x } else { x - next };
        x = next;
        if diff < Decimal::new(1, 10) {
            break;
        }
    }
    x
}

impl Signal for StdDev {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() > self.period {
            self.history.pop_front();
        }
        if self.history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let n_dec = Decimal::from(self.period as u64);
        let mean: Decimal = self.history.iter().copied().sum::<Decimal>() / n_dec;
        let variance: Decimal = self
            .history
            .iter()
            .map(|v| { let d = *v - mean; d * d })
            .sum::<Decimal>()
            / n_dec;
        Ok(SignalValue::Scalar(decimal_sqrt(variance)))
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

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_stddev_period_0_error() {
        assert!(StdDev::new("sd", 0).is_err());
    }

    #[test]
    fn test_stddev_unavailable_before_period() {
        let mut sd = StdDev::new("sd3", 3).unwrap();
        assert_eq!(sd.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sd.update_bar(&bar("110")).unwrap(), SignalValue::Unavailable);
        assert!(sd.update_bar(&bar("120")).unwrap().is_scalar());
    }

    #[test]
    fn test_stddev_constant_prices_is_zero() {
        let mut sd = StdDev::new("sd3", 3).unwrap();
        sd.update_bar(&bar("100")).unwrap();
        sd.update_bar(&bar("100")).unwrap();
        let v = sd.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_stddev_reset_clears_state() {
        let mut sd = StdDev::new("sd2", 2).unwrap();
        sd.update_bar(&bar("100")).unwrap();
        sd.update_bar(&bar("110")).unwrap();
        assert!(sd.is_ready());
        sd.reset();
        assert!(!sd.is_ready());
        assert_eq!(sd.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_stddev_positive_for_varying_prices() {
        let mut sd = StdDev::new("sd3", 3).unwrap();
        sd.update_bar(&bar("90")).unwrap();
        sd.update_bar(&bar("100")).unwrap();
        let v = sd.update_bar(&bar("110")).unwrap();
        // mean=100, var=((90-100)²+(100-100)²+(110-100)²)/3 = (100+0+100)/3 ≈ 66.67 → stddev ≈ 8.16
        match v {
            SignalValue::Scalar(d) => assert!(d > dec!(0), "stddev should be positive"),
            _ => panic!("expected Scalar"),
        }
    }
}
