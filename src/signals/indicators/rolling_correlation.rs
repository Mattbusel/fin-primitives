//! Rolling Correlation — Pearson correlation between close price and volume over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Correlation — Pearson correlation between close and volume over N bars.
///
/// Measures the linear relationship between price and volume:
/// - **Near +1**: volume rises with price (price-volume confirmation).
/// - **Near −1**: volume rises as price falls (divergence / distribution).
/// - **Near 0**: no linear relationship.
///
/// Uses population Pearson correlation coefficient. Returns [`SignalValue::Unavailable`]
/// until `period` bars have been seen, or when standard deviation of either series is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingCorrelation;
/// use fin_primitives::signals::Signal;
/// let rc = RollingCorrelation::new("rc_20", 20).unwrap();
/// assert_eq!(rc.period(), 20);
/// ```
pub struct RollingCorrelation {
    name: String,
    period: usize,
    window: VecDeque<(Decimal, Decimal)>, // (close, volume)
}

impl RollingCorrelation {
    /// Constructs a new `RollingCorrelation`.
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
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RollingCorrelation {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back((bar.close, bar.volume));
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let xs: Vec<f64> = self.window.iter().filter_map(|(c, _)| c.to_f64()).collect();
        let ys: Vec<f64> = self.window.iter().filter_map(|(_, v)| v.to_f64()).collect();

        if xs.len() != self.period || ys.len() != self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mean_x = xs.iter().sum::<f64>() / n;
        let mean_y = ys.iter().sum::<f64>() / n;

        let var_x: f64 = xs.iter().map(|x| (x - mean_x) * (x - mean_x)).sum::<f64>() / n;
        let var_y: f64 = ys.iter().map(|y| (y - mean_y) * (y - mean_y)).sum::<f64>() / n;

        let std_x = var_x.sqrt();
        let std_y = var_y.sqrt();

        if std_x == 0.0 || std_y == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let cov: f64 = xs
            .iter()
            .zip(ys.iter())
            .map(|(x, y)| (x - mean_x) * (y - mean_y))
            .sum::<f64>()
            / n;

        let corr = (cov / (std_x * std_y)).clamp(-1.0, 1.0);

        Decimal::try_from(corr)
            .map(SignalValue::Scalar)
            .or(Ok(SignalValue::Unavailable))
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str, vol: &str) -> OhlcvBar {
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: cp, low: cp, close: cp,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rc_invalid_period() {
        assert!(RollingCorrelation::new("rc", 0).is_err());
        assert!(RollingCorrelation::new("rc", 1).is_err());
    }

    #[test]
    fn test_rc_unavailable_before_period() {
        let mut s = RollingCorrelation::new("rc", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100","1000")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("102","2000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rc_perfect_positive_correlation() {
        let mut s = RollingCorrelation::new("rc", 3).unwrap();
        // Price and volume both increasing together
        s.update_bar(&bar("100","1000")).unwrap();
        s.update_bar(&bar("102","2000")).unwrap();
        let v = s.update_bar(&bar("104","3000")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(1)).abs() < dec!(0.001), "expected correlation ~1, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rc_perfect_negative_correlation() {
        let mut s = RollingCorrelation::new("rc", 3).unwrap();
        // Price and volume move inversely
        s.update_bar(&bar("100","3000")).unwrap();
        s.update_bar(&bar("102","2000")).unwrap();
        let v = s.update_bar(&bar("104","1000")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r + dec!(1)).abs() < dec!(0.001), "expected correlation ~-1, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rc_output_in_range() {
        let mut s = RollingCorrelation::new("rc", 4).unwrap();
        let data = [("100","1000"),("102","2500"),("101","800"),("103","1500"),("104","3000")];
        for (c, v) in &data {
            if let SignalValue::Scalar(r) = s.update_bar(&bar(c, v)).unwrap() {
                assert!(r >= dec!(-1) && r <= dec!(1), "correlation out of [-1,1]: {r}");
            }
        }
    }

    #[test]
    fn test_rc_reset() {
        let mut s = RollingCorrelation::new("rc", 3).unwrap();
        for _ in 0..3 { s.update_bar(&bar("100","1000")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
