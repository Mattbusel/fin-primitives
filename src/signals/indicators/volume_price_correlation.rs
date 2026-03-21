//! Volume-Price Correlation indicator.
//!
//! Measures the rolling Pearson correlation between closing price and volume,
//! capturing whether rising prices are accompanied by rising volume (confirming
//! trend) or diverging volume (potential reversal warning).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Rolling Pearson correlation between `close` and `volume`.
///
/// A positive value means higher prices coincide with heavier volume (healthy
/// trend). A negative value means higher prices come on lighter volume (trend
/// divergence). Near zero indicates no relationship.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// when either series has zero variance.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 2`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumePriceCorrelation;
/// use fin_primitives::signals::Signal;
///
/// let vpc = VolumePriceCorrelation::new("vpc", 20).unwrap();
/// assert_eq!(vpc.period(), 20);
/// assert!(!vpc.is_ready());
/// ```
pub struct VolumePriceCorrelation {
    name: String,
    period: usize,
    window: VecDeque<(Decimal, Decimal)>, // (close, volume)
}

impl VolumePriceCorrelation {
    /// Constructs a new `VolumePriceCorrelation`.
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

impl crate::signals::Signal for VolumePriceCorrelation {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back((bar.close, bar.volume));
        if self.window.len() > self.period {
            self.window.pop_front();
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let mut sum_x = 0.0_f64;
        let mut sum_y = 0.0_f64;
        for (c, v) in &self.window {
            sum_x += c.to_f64().unwrap_or(0.0);
            sum_y += v.to_f64().unwrap_or(0.0);
        }
        let mean_x = sum_x / n;
        let mean_y = sum_y / n;

        let mut cov = 0.0_f64;
        let mut var_x = 0.0_f64;
        let mut var_y = 0.0_f64;
        for (c, v) in &self.window {
            let dx = c.to_f64().unwrap_or(0.0) - mean_x;
            let dy = v.to_f64().unwrap_or(0.0) - mean_y;
            cov += dx * dy;
            var_x += dx * dx;
            var_y += dy * dy;
        }

        let denom = (var_x * var_y).sqrt();
        if denom == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let corr = cov / denom;
        let result = Decimal::try_from(corr).unwrap_or(Decimal::ZERO);
        Ok(SignalValue::Scalar(result))
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

    fn bar(close: &str, vol: &str) -> OhlcvBar {
        let c = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: c, high: c, low: c, close: c,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vpc_invalid_period() {
        assert!(VolumePriceCorrelation::new("vpc", 0).is_err());
        assert!(VolumePriceCorrelation::new("vpc", 1).is_err());
    }

    #[test]
    fn test_vpc_unavailable_during_warmup() {
        let mut vpc = VolumePriceCorrelation::new("vpc", 5).unwrap();
        for _ in 0..4 {
            assert_eq!(vpc.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vpc_perfect_positive_correlation() {
        // price and volume rise together → correlation = 1
        let mut vpc = VolumePriceCorrelation::new("vpc", 5).unwrap();
        let pairs = [("100", "100"), ("110", "200"), ("120", "300"),
                     ("130", "400"), ("140", "500")];
        let mut last = SignalValue::Unavailable;
        for (c, v) in pairs {
            last = vpc.update_bar(&bar(c, v)).unwrap();
        }
        if let SignalValue::Scalar(s) = last {
            assert!(s > dec!(0.99), "expected ~1.0: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vpc_zero_variance_unavailable() {
        // All closes identical → var_x = 0
        let mut vpc = VolumePriceCorrelation::new("vpc", 3).unwrap();
        vpc.update_bar(&bar("100", "100")).unwrap();
        vpc.update_bar(&bar("100", "200")).unwrap();
        let v = vpc.update_bar(&bar("100", "300")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_vpc_result_in_range() {
        let mut vpc = VolumePriceCorrelation::new("vpc", 4).unwrap();
        let pairs = [("100", "500"), ("110", "100"), ("90", "800"), ("105", "200")];
        let mut last = SignalValue::Unavailable;
        for (c, v) in pairs {
            last = vpc.update_bar(&bar(c, v)).unwrap();
        }
        if let SignalValue::Scalar(s) = last {
            assert!(s >= dec!(-1) && s <= dec!(1), "out of [-1,1]: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vpc_reset() {
        let mut vpc = VolumePriceCorrelation::new("vpc", 3).unwrap();
        for _ in 0..3 {
            vpc.update_bar(&bar("100", "1000")).unwrap();
        }
        assert!(vpc.is_ready());
        vpc.reset();
        assert!(!vpc.is_ready());
    }
}
