//! Mean Reversion Score indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Mean Reversion Score — distance from mean normalized by standard deviation,
/// then inverted to score reversion potential.
///
/// ```text
/// z_score = (close − mean(close, period)) / std_dev(close, period)
/// output  = −z_score   [positive = below mean (buy signal), negative = above]
/// ```
///
/// A positive score indicates price is below its mean (potential upward reversion);
/// a negative score indicates price is above its mean (potential downward reversion).
/// Returns 0 when std_dev is zero (flat market).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MeanReversionScore;
/// use fin_primitives::signals::Signal;
///
/// let mrs = MeanReversionScore::new("mrs", 20).unwrap();
/// assert_eq!(mrs.period(), 20);
/// ```
pub struct MeanReversionScore {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl MeanReversionScore {
    /// Creates a new `MeanReversionScore`.
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

impl Signal for MeanReversionScore {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period { self.closes.pop_front(); }
        if self.closes.len() < self.period { return Ok(SignalValue::Unavailable); }

        let ys: Vec<f64> = self.closes.iter()
            .filter_map(|c| c.to_f64())
            .collect();
        let n = ys.len() as f64;
        let mean = ys.iter().sum::<f64>() / n;
        let variance = ys.iter().map(|&y| (y - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        let last = *ys.last().unwrap();

        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let z = (last - mean) / std_dev;
        let score = -z; // positive when below mean = reversion opportunity
        Ok(SignalValue::Scalar(
            Decimal::try_from(score).unwrap_or(Decimal::ZERO)
        ))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period }
    fn period(&self) -> usize { self.period }

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
    fn test_mrs_invalid() {
        assert!(MeanReversionScore::new("m", 0).is_err());
        assert!(MeanReversionScore::new("m", 1).is_err());
    }

    #[test]
    fn test_mrs_unavailable_before_warmup() {
        let mut m = MeanReversionScore::new("m", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(m.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_mrs_flat_is_zero() {
        let mut m = MeanReversionScore::new("m", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = m.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_mrs_below_mean_positive() {
        // window = [100, 110, 90]; mean = 100; close = 90 → z = (90-100)/σ < 0 → score > 0
        let mut m = MeanReversionScore::new("m", 3).unwrap();
        m.update_bar(&bar("100")).unwrap();
        m.update_bar(&bar("110")).unwrap();
        if let SignalValue::Scalar(v) = m.update_bar(&bar("90")).unwrap() {
            assert!(v > dec!(0), "expected positive score (below mean), got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_mrs_above_mean_negative() {
        // window = [100, 90, 110]; mean = 100; close = 110 → z = (110-100)/σ > 0 → score < 0
        let mut m = MeanReversionScore::new("m", 3).unwrap();
        m.update_bar(&bar("100")).unwrap();
        m.update_bar(&bar("90")).unwrap();
        if let SignalValue::Scalar(v) = m.update_bar(&bar("110")).unwrap() {
            assert!(v < dec!(0), "expected negative score (above mean), got {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_mrs_reset() {
        let mut m = MeanReversionScore::new("m", 3).unwrap();
        for _ in 0..5 { m.update_bar(&bar("100")).unwrap(); }
        assert!(m.is_ready());
        m.reset();
        assert!(!m.is_ready());
    }
}
