//! Hurst Exponent indicator (R/S analysis approximation).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Hurst Exponent — measures the memory (persistence) of a price series using
/// Rescaled Range (R/S) analysis.
///
/// Interpretation:
/// - H > 0.5 → trending (persistent) market
/// - H < 0.5 → mean-reverting market
/// - H ≈ 0.5 → random walk
///
/// The output is a [`SignalValue::Scalar`] in [0, 1].
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HurstExponent;
/// use fin_primitives::signals::Signal;
///
/// let h = HurstExponent::new("hurst", 20).unwrap();
/// assert_eq!(h.period(), 20);
/// ```
pub struct HurstExponent {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl HurstExponent {
    /// Constructs a new `HurstExponent`.
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
            closes: VecDeque::with_capacity(period + 1),
        })
    }

    fn compute(closes: &VecDeque<Decimal>) -> Option<Decimal> {
        let n = closes.len();
        if n < 2 {
            return None;
        }
        let cf: Vec<f64> = closes.iter().filter_map(|c| c.to_f64()).collect();
        if cf.len() != n {
            return None;
        }
        // Compute returns
        let returns: Vec<f64> = cf.windows(2).map(|w| w[1] - w[0]).collect();
        let m = returns.len();
        if m == 0 {
            return None;
        }
        let mean: f64 = returns.iter().sum::<f64>() / m as f64;

        // Cumulative deviation
        let mut cum = 0.0f64;
        let mut max_cum = f64::NEG_INFINITY;
        let mut min_cum = f64::INFINITY;
        for r in &returns {
            cum += r - mean;
            if cum > max_cum { max_cum = cum; }
            if cum < min_cum { min_cum = cum; }
        }
        let range_r = max_cum - min_cum;

        let variance: f64 = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / m as f64;
        let std_s = variance.sqrt();

        if std_s == 0.0 || range_r == 0.0 {
            return Decimal::from_f64(0.5);
        }
        let rs = range_r / std_s;
        let h = rs.ln() / (m as f64).ln();
        Decimal::from_f64(h.clamp(0.0, 1.0))
    }
}

impl Signal for HurstExponent {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }
        match Self::compute(&self.closes) {
            Some(h) => Ok(SignalValue::Scalar(h)),
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
    fn test_hurst_invalid_period() {
        assert!(HurstExponent::new("h", 0).is_err());
        assert!(HurstExponent::new("h", 1).is_err());
    }

    #[test]
    fn test_hurst_unavailable_before_warm_up() {
        let mut h = HurstExponent::new("h", 5).unwrap();
        for _ in 0..5 {
            assert_eq!(h.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!h.is_ready());
    }

    #[test]
    fn test_hurst_ready_after_warm_up() {
        let mut h = HurstExponent::new("h", 5).unwrap();
        for i in 0u32..6 {
            h.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(h.is_ready());
    }

    #[test]
    fn test_hurst_trending_above_half() {
        let mut h = HurstExponent::new("h", 10).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..11 {
            last = h.update_bar(&bar(&(100 + i * 5).to_string())).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0.5), "trending series should have H > 0.5, got {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hurst_output_in_range() {
        let mut h = HurstExponent::new("h", 8).unwrap();
        // alternating prices → mean-reverting
        let prices = ["100","99","100","99","100","99","100","99","100"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = h.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v >= dec!(0) && v <= dec!(1));
        }
    }

    #[test]
    fn test_hurst_reset() {
        let mut h = HurstExponent::new("h", 5).unwrap();
        for i in 0u32..6 { h.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        assert!(h.is_ready());
        h.reset();
        assert!(!h.is_ready());
    }
}
