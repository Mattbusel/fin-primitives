//! Trend Angle indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Angle — measures the angle (in degrees) of the linear regression line
/// fitted to the last `period` closing prices.
///
/// The slope is computed in units of `price / bar`, then converted to an angle
/// via `atan(slope)`.  Positive angle = uptrend; negative = downtrend; near zero = flat.
///
/// ```text
/// angle = atan(b) × (180 / π)
/// ```
/// where `b` is the OLS slope of closes regressed on bar indices 0..period-1.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendAngle;
/// use fin_primitives::signals::Signal;
///
/// let ta = TrendAngle::new("angle20", 20).unwrap();
/// assert_eq!(ta.period(), 20);
/// ```
pub struct TrendAngle {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl TrendAngle {
    /// Creates a new `TrendAngle`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for TrendAngle {
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
        let ys: Vec<f64> = self.closes.iter()
            .map(|c| c.to_f64().unwrap_or(0.0))
            .collect();
        let x_mean = (n - 1.0) / 2.0;
        let y_mean = ys.iter().sum::<f64>() / n;
        let ss_xx: f64 = (0..self.period).map(|i| (i as f64 - x_mean).powi(2)).sum();
        let ss_xy: f64 = ys.iter().enumerate()
            .map(|(i, y)| (i as f64 - x_mean) * (y - y_mean))
            .sum();

        let slope = if ss_xx == 0.0 { 0.0 } else { ss_xy / ss_xx };
        let angle_deg = slope.atan().to_degrees();

        Ok(SignalValue::Scalar(
            Decimal::try_from(angle_deg).unwrap_or(Decimal::ZERO),
        ))
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
    fn test_trend_angle_invalid_period() {
        assert!(TrendAngle::new("a", 0).is_err());
        assert!(TrendAngle::new("a", 1).is_err());
    }

    #[test]
    fn test_trend_angle_unavailable_before_period() {
        let mut ta = TrendAngle::new("a", 4).unwrap();
        for _ in 0..3 {
            assert_eq!(ta.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_trend_angle_flat_price_is_zero() {
        let mut ta = TrendAngle::new("a", 4).unwrap();
        for _ in 0..4 { ta.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = ta.update_bar(&bar("100")).unwrap() {
            assert_eq!(v, dec!(0));
        }
    }

    #[test]
    fn test_trend_angle_uptrend_positive() {
        let mut ta = TrendAngle::new("a", 4).unwrap();
        // Rising prices → positive slope → positive angle
        for c in &["100", "101", "102", "103"] {
            ta.update_bar(&bar(c)).unwrap();
        }
        if let SignalValue::Scalar(v) = ta.update_bar(&bar("104")).unwrap() {
            assert!(v > dec!(0), "uptrend angle should be positive, got {v}");
        }
    }

    #[test]
    fn test_trend_angle_downtrend_negative() {
        let mut ta = TrendAngle::new("a", 4).unwrap();
        // Falling prices → negative slope → negative angle
        for c in &["104", "103", "102", "101"] {
            ta.update_bar(&bar(c)).unwrap();
        }
        if let SignalValue::Scalar(v) = ta.update_bar(&bar("100")).unwrap() {
            assert!(v < dec!(0), "downtrend angle should be negative, got {v}");
        }
    }

    #[test]
    fn test_trend_angle_reset() {
        let mut ta = TrendAngle::new("a", 4).unwrap();
        for _ in 0..5 { ta.update_bar(&bar("100")).unwrap(); }
        assert!(ta.is_ready());
        ta.reset();
        assert!(!ta.is_ready());
    }
}
