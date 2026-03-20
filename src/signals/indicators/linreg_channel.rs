//! Linear Regression Channel indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Linear Regression Channel — fits an OLS line to the last `period` closes and
/// returns the midline (predicted value at the current bar).  Upper and lower
/// channel lines are accessible via accessors.
///
/// ```text
/// linreg_mid[i] = a + b × (period - 1)      // predicted value at the latest bar
/// upper[i]      = linreg_mid[i] + multiplier × StdDev
/// lower[i]      = linreg_mid[i] - multiplier × StdDev
/// ```
///
/// Returns [`SignalValue::Scalar`] (midline) once `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LinRegChannel;
/// use fin_primitives::signals::Signal;
///
/// let lrc = LinRegChannel::new("lrc20", 20, 2.0).unwrap();
/// assert_eq!(lrc.period(), 20);
/// ```
pub struct LinRegChannel {
    name: String,
    period: usize,
    multiplier: f64,
    closes: VecDeque<Decimal>,
    upper: Option<Decimal>,
    lower: Option<Decimal>,
}

impl LinRegChannel {
    /// Creates a new `LinRegChannel`.
    ///
    /// * `period`     — number of bars for the regression
    /// * `multiplier` — standard deviation multiplier for channel width (e.g. `2.0`)
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize, multiplier: f64) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            closes: VecDeque::with_capacity(period),
            upper: None,
            lower: None,
        })
    }

    /// Returns the upper channel line value from the most recent bar.
    pub fn upper(&self) -> Option<Decimal> { self.upper }
    /// Returns the lower channel line value from the most recent bar.
    pub fn lower(&self) -> Option<Decimal> { self.lower }
    /// Returns the channel width (`upper - lower`), if available.
    pub fn channel_width(&self) -> Option<Decimal> {
        Some(self.upper? - self.lower?)
    }
}

impl Signal for LinRegChannel {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let xs: Vec<f64> = (0..self.period).map(|i| i as f64).collect();
        let ys: Vec<f64> = self.closes.iter()
            .map(|c| c.to_f64().unwrap_or(0.0))
            .collect();

        let x_mean = xs.iter().sum::<f64>() / n;
        let y_mean = ys.iter().sum::<f64>() / n;

        let ss_xx: f64 = xs.iter().map(|x| (x - x_mean).powi(2)).sum();
        let ss_xy: f64 = xs.iter().zip(ys.iter()).map(|(x, y)| (x - x_mean) * (y - y_mean)).sum();

        let b = if ss_xx == 0.0 { 0.0 } else { ss_xy / ss_xx };
        let a = y_mean - b * x_mean;
        let midline = a + b * (self.period as f64 - 1.0);

        // Residual standard deviation
        let residuals: f64 = xs.iter().zip(ys.iter())
            .map(|(x, y)| (y - (a + b * x)).powi(2))
            .sum::<f64>()
            / n;
        let std_dev = residuals.sqrt();

        let upper = midline + self.multiplier * std_dev;
        let lower = midline - self.multiplier * std_dev;

        let mid_d = Decimal::try_from(midline).unwrap_or(bar.close);
        let upper_d = Decimal::try_from(upper).unwrap_or(bar.close);
        let lower_d = Decimal::try_from(lower).unwrap_or(bar.close);

        self.upper = Some(upper_d);
        self.lower = Some(lower_d);

        Ok(SignalValue::Scalar(mid_d))
    }

    fn is_ready(&self) -> bool {
        self.upper.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.upper = None;
        self.lower = None;
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
    fn test_lrc_invalid_period() {
        assert!(LinRegChannel::new("l", 0, 2.0).is_err());
        assert!(LinRegChannel::new("l", 1, 2.0).is_err());
    }

    #[test]
    fn test_lrc_unavailable_before_period() {
        let mut lrc = LinRegChannel::new("l", 4, 2.0).unwrap();
        for _ in 0..3 {
            assert_eq!(lrc.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!lrc.is_ready());
    }

    #[test]
    fn test_lrc_flat_mid_equals_price() {
        let mut lrc = LinRegChannel::new("l", 4, 2.0).unwrap();
        for _ in 0..4 { lrc.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = lrc.update_bar(&bar("100")).unwrap() {
            let diff = (v - dec!(100)).abs();
            assert!(diff < dec!(0.001), "expected midline ~100, got {v}");
        }
        // Flat price → zero residuals → zero channel width
        assert_eq!(lrc.channel_width().unwrap(), dec!(0));
    }

    #[test]
    fn test_lrc_upper_above_lower() {
        let mut lrc = LinRegChannel::new("l", 5, 2.0).unwrap();
        for c in &["100", "102", "99", "103", "101"] {
            lrc.update_bar(&bar(c)).unwrap();
        }
        assert!(lrc.is_ready());
        let upper = lrc.upper().unwrap();
        let lower = lrc.lower().unwrap();
        assert!(upper >= lower, "upper should be >= lower");
    }

    #[test]
    fn test_lrc_reset() {
        let mut lrc = LinRegChannel::new("l", 3, 2.0).unwrap();
        for _ in 0..5 { lrc.update_bar(&bar("100")).unwrap(); }
        assert!(lrc.is_ready());
        lrc.reset();
        assert!(!lrc.is_ready());
        assert!(lrc.upper().is_none());
        assert!(lrc.lower().is_none());
    }
}
