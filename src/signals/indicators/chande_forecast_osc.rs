//! Chande Forecast Oscillator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Chande Forecast Oscillator (CFO) — measures momentum as the difference
/// between the current close and the linear regression forecast, expressed as
/// a percentage of close:
///
/// ```text
/// CFO = (close - linreg_forecast(close, n)) / close × 100
/// ```
///
/// - Positive values → close is above the linear trend (upward momentum)
/// - Negative values → close is below the linear trend (downward momentum)
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ChandeForecastOsc;
/// use fin_primitives::signals::Signal;
///
/// let cfo = ChandeForecastOsc::new("cfo", 14).unwrap();
/// assert_eq!(cfo.period(), 14);
/// ```
pub struct ChandeForecastOsc {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl ChandeForecastOsc {
    /// Constructs a new `ChandeForecastOsc`.
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
        })
    }

    /// Computes the least-squares linear regression forecast for the last value.
    fn linreg_forecast(closes: &VecDeque<Decimal>) -> Option<f64> {
        let n = closes.len();
        if n < 2 { return None; }
        let nf = n as f64;
        // x = 0, 1, ..., n-1
        let sum_x = nf * (nf - 1.0) / 2.0;
        let sum_x2 = nf * (nf - 1.0) * (2.0 * nf - 1.0) / 6.0;
        let ys: Vec<f64> = closes.iter().filter_map(|c| c.to_f64()).collect();
        if ys.len() != n { return None; }
        let sum_y: f64 = ys.iter().sum();
        let sum_xy: f64 = ys.iter().enumerate().map(|(i, y)| i as f64 * y).sum();
        let denom = nf * sum_x2 - sum_x * sum_x;
        if denom == 0.0 { return Some(ys[n - 1]); }
        let slope = (nf * sum_xy - sum_x * sum_y) / denom;
        let intercept = (sum_y - slope * sum_x) / nf;
        // Forecast for x = n-1 (the last bar's position)
        Some(slope * (nf - 1.0) + intercept)
    }
}

impl Signal for ChandeForecastOsc {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }
        if self.closes.len() < self.period { return Ok(SignalValue::Unavailable); }

        let close_f = match bar.close.to_f64() {
            Some(f) if f != 0.0 => f,
            _ => return Ok(SignalValue::Unavailable),
        };
        let forecast = match Self::linreg_forecast(&self.closes) {
            Some(f) => f,
            None => return Ok(SignalValue::Unavailable),
        };
        let cfo = (close_f - forecast) / close_f * 100.0;
        match Decimal::from_f64(cfo) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn reset(&mut self) {
        self.closes.clear();
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
    fn test_cfo_invalid_period() {
        assert!(ChandeForecastOsc::new("c", 0).is_err());
        assert!(ChandeForecastOsc::new("c", 1).is_err());
    }

    #[test]
    fn test_cfo_unavailable_before_warm_up() {
        let mut cfo = ChandeForecastOsc::new("c", 5).unwrap();
        for _ in 0..4 {
            assert_eq!(cfo.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_cfo_linear_trend_near_zero() {
        // On a perfectly linear trend, the last close equals the linreg forecast → CFO ≈ 0
        let mut cfo = ChandeForecastOsc::new("c", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..5 {
            last = cfo.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v.abs() < dec!(0.01), "linear trend should give CFO ≈ 0, got {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cfo_upside_acceleration_positive() {
        // Prices accelerating upward → close exceeds linear forecast → positive CFO
        let prices = ["100","101","103","106","110"];
        let mut cfo = ChandeForecastOsc::new("c", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = cfo.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "accelerating uptrend should give positive CFO, got {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cfo_reset() {
        let mut cfo = ChandeForecastOsc::new("c", 5).unwrap();
        for i in 0u32..5 { cfo.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        assert!(cfo.is_ready());
        cfo.reset();
        assert!(!cfo.is_ready());
    }
}
