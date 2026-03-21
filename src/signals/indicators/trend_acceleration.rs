//! Trend Acceleration indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Acceleration.
///
/// Measures the rate of change of the linear regression slope over a rolling
/// window. Positive values indicate the trend is speeding up; negative values
/// indicate it is slowing down or reversing.
///
/// Method:
/// 1. Compute the linear regression slope of the last `period` closes.
/// 2. Store the last two slopes (current and previous).
/// 3. Output = current_slope − prev_slope.
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendAcceleration;
/// use fin_primitives::signals::Signal;
/// let ta = TrendAcceleration::new("ta_10", 10).unwrap();
/// assert_eq!(ta.period(), 10);
/// ```
pub struct TrendAcceleration {
    name: String,
    period: usize,
    closes: VecDeque<f64>,
    prev_slope: Option<f64>,
}

impl TrendAcceleration {
    /// Constructs a new `TrendAcceleration` with the given name and period.
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
            prev_slope: None,
        })
    }

    /// Compute linear regression slope for a slice of values.
    fn linreg_slope(data: &VecDeque<f64>) -> f64 {
        let n = data.len() as f64;
        let x_mean = (n - 1.0) / 2.0;
        let y_mean = data.iter().sum::<f64>() / n;
        let mut num = 0.0_f64;
        let mut den = 0.0_f64;
        for (i, &y) in data.iter().enumerate() {
            let x = i as f64;
            num += (x - x_mean) * (y - y_mean);
            den += (x - x_mean).powi(2);
        }
        if den == 0.0 { 0.0 } else { num / den }
    }
}

impl Signal for TrendAcceleration {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;
        let c = bar.close.to_f64().unwrap_or(0.0);
        self.closes.push_back(c);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let slope = Self::linreg_slope(&self.closes);
        let result = match self.prev_slope {
            None => {
                self.prev_slope = Some(slope);
                SignalValue::Unavailable
            }
            Some(prev) => {
                let accel = slope - prev;
                self.prev_slope = Some(slope);
                match Decimal::try_from(accel) {
                    Ok(d) => SignalValue::Scalar(d),
                    Err(_) => return Err(FinError::ArithmeticOverflow),
                }
            }
        };
        Ok(result)
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period && self.prev_slope.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.prev_slope = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_too_small_fails() {
        assert!(TrendAcceleration::new("ta", 1).is_err());
        assert!(TrendAcceleration::new("ta", 0).is_err());
    }

    #[test]
    fn test_unavailable_before_warmup() {
        let mut ta = TrendAcceleration::new("ta", 3).unwrap();
        for _ in 0..3 {
            let v = ta.update_bar(&bar("10")).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ready_after_warmup() {
        let mut ta = TrendAcceleration::new("ta", 3).unwrap();
        for _ in 0..4 {
            ta.update_bar(&bar("10")).unwrap();
        }
        assert!(ta.is_ready());
    }

    #[test]
    fn test_constant_trend_zero_acceleration() {
        // Perfectly linear uptrend: slope constant → acceleration = 0
        let mut ta = TrendAcceleration::new("ta", 3).unwrap();
        for i in 1..=5 {
            ta.update_bar(&bar(&(i * 10).to_string())).unwrap();
        }
        let v = ta.update_bar(&bar("60")).unwrap();
        if let SignalValue::Scalar(s) = v {
            // Acceleration of a perfectly linear series should be near 0
            assert!(s.abs() < dec!(0.001), "expected near-zero acceleration, got {}", s);
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut ta = TrendAcceleration::new("ta", 3).unwrap();
        for _ in 0..5 {
            ta.update_bar(&bar("10")).unwrap();
        }
        ta.reset();
        assert!(!ta.is_ready());
        assert!(ta.prev_slope.is_none());
    }
}
