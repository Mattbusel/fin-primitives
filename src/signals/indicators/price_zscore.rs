//! Price Z-Score indicator -- rolling z-score of close price.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Z-Score -- how many standard deviations the current close is from its
/// rolling N-period mean.
///
/// ```text
/// mean[t]    = SMA(close, period)
/// stddev[t]  = sample stddev of close over period
/// zscore[t]  = (close[t] - mean[t]) / stddev[t]
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or when
/// standard deviation is zero (all prices identical).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceZScore;
/// use fin_primitives::signals::Signal;
/// let pz = PriceZScore::new("pz", 20).unwrap();
/// assert_eq!(pz.period(), 20);
/// ```
pub struct PriceZScore {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceZScore {
    /// Constructs a new `PriceZScore`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2` (need at least 2 values for stddev).
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceZScore {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }

        #[allow(clippy::cast_possible_truncation)]
        let n = Decimal::from(self.period as u32);
        let mean = self.sum / n;

        // Sample variance
        let variance = self.window.iter()
            .map(|v| {
                let diff = *v - mean;
                diff * diff
            })
            .fold(Decimal::ZERO, |acc, v| acc + v)
            / (n - Decimal::ONE);

        if variance <= Decimal::ZERO {
            return Ok(SignalValue::Unavailable);
        }

        // sqrt via Newton-Raphson on Decimal
        let variance_f: f64 = variance.to_string().parse().unwrap_or(f64::NAN);
        if variance_f.is_nan() { return Ok(SignalValue::Unavailable); }
        let stddev_f = variance_f.sqrt();
        let stddev = match Decimal::try_from(stddev_f) {
            Ok(d) if !d.is_zero() => d,
            _ => return Ok(SignalValue::Unavailable),
        };

        Ok(SignalValue::Scalar((bar.close - mean) / stddev))
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
    fn test_pz_period_less_than_2_error() { assert!(PriceZScore::new("p", 1).is_err()); }
    #[test]
    fn test_pz_period_0_error() { assert!(PriceZScore::new("p", 0).is_err()); }

    #[test]
    fn test_pz_unavailable_before_period() {
        let mut p = PriceZScore::new("p", 3).unwrap();
        assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pz_constant_price_unavailable() {
        // stddev = 0 for constant prices
        let mut p = PriceZScore::new("p", 3).unwrap();
        for _ in 0..3 { p.update_bar(&bar("100")).unwrap(); }
        assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pz_mean_price_is_zero() {
        // close == mean -> z-score = 0
        let mut p = PriceZScore::new("p", 3).unwrap();
        p.update_bar(&bar("90")).unwrap();
        p.update_bar(&bar("110")).unwrap();
        // Third bar at 100 (mean of 90,110,100 = 100). z-score(100) should be 0.
        let v = p.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(z) = v {
            assert!(z.abs() < dec!(0.001), "expected ~0 z-score at mean, got {z}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pz_reset() {
        let mut p = PriceZScore::new("p", 3).unwrap();
        for _ in 0..3 { p.update_bar(&bar("100")).unwrap(); }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
