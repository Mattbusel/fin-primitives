//! Volatility-Adjusted Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::VecDeque;

/// Volatility-Adjusted Momentum — N-period price change divided by its
/// standard deviation (a Sharpe-ratio-like momentum score).
///
/// ```text
/// momentum   = close_t − close_{t−period}
/// std_dev    = std(close, period+1)
/// output     = momentum / std_dev
/// ```
///
/// Positive values indicate upward momentum relative to volatility;
/// negative indicates downward. Normalised momentum makes signals
/// comparable across instruments with different volatility profiles.
/// Returns 0 when std_dev is zero (flat market).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityAdjustedMomentum;
/// use fin_primitives::signals::Signal;
///
/// let vam = VolatilityAdjustedMomentum::new("vam", 10).unwrap();
/// assert_eq!(vam.period(), 10);
/// ```
pub struct VolatilityAdjustedMomentum {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl VolatilityAdjustedMomentum {
    /// Creates a new `VolatilityAdjustedMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for VolatilityAdjustedMomentum {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 { self.closes.pop_front(); }
        if self.closes.len() < self.period + 1 { return Ok(SignalValue::Unavailable); }

        let closes: Vec<f64> = self.closes.iter()
            .filter_map(|c| c.to_f64())
            .collect();

        if closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let n = closes.len() as f64;
        let mean = closes.iter().sum::<f64>() / n;
        let variance = closes.iter().map(|&c| (c - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        let momentum = closes[self.period] - closes[0];

        if std_dev == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let result = momentum / std_dev;
        Ok(SignalValue::Scalar(
            Decimal::try_from(result).unwrap_or(Decimal::ZERO)
        ))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }
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
    fn test_vam_invalid() {
        assert!(VolatilityAdjustedMomentum::new("v", 0).is_err());
        assert!(VolatilityAdjustedMomentum::new("v", 1).is_err());
    }

    #[test]
    fn test_vam_unavailable_before_warmup() {
        let mut v = VolatilityAdjustedMomentum::new("v", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(v.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vam_flat_is_zero() {
        // Flat: momentum=0 and std_dev=0 → returns 0
        let mut v = VolatilityAdjustedMomentum::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..8 { last = v.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(val) = last {
            assert_eq!(val, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vam_uptrend_positive() {
        // Rising prices → positive momentum / std_dev > 0
        let mut v = VolatilityAdjustedMomentum::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..10 {
            let p = format!("{}", 100 + i);
            last = v.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(val) = last {
            assert!(val > dec!(0), "expected positive, got {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vam_downtrend_negative() {
        let mut v = VolatilityAdjustedMomentum::new("v", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..10 {
            let p = format!("{}", 200 - i);
            last = v.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(val) = last {
            assert!(val < dec!(0), "expected negative, got {val}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_vam_reset() {
        let mut v = VolatilityAdjustedMomentum::new("v", 3).unwrap();
        for i in 0u32..10 {
            let p = format!("{}", 100 + i);
            v.update_bar(&bar(&p)).unwrap();
        }
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
