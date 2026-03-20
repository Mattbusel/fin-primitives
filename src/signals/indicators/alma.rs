//! Arnaud Legoux Moving Average (ALMA).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Arnaud Legoux Moving Average — Gaussian-weighted moving average.
///
/// Uses a bell-curve (Gaussian) weighting shifted by `offset` along the window,
/// giving more weight to recent prices while reducing lag.
///
/// Parameters:
/// - `period`: window length (default 9)
/// - `sigma`: Gaussian width (default 6.0 — higher = sharper peak, less smoothing)
/// - `offset`: centre shift in [0,1] (default 0.85 — 1.0 = full right-shift, low lag)
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Alma;
/// use fin_primitives::signals::Signal;
///
/// let a = Alma::new("alma9", 9, 0.85, 6.0).unwrap();
/// assert_eq!(a.period(), 9);
/// assert!(!a.is_ready());
/// ```
pub struct Alma {
    name: String,
    period: usize,
    weights: Vec<Decimal>,
    closes: VecDeque<Decimal>,
}

impl Alma {
    /// Constructs a new `Alma`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if `sigma <= 0`.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        offset: f64,
        sigma: f64,
    ) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        if sigma <= 0.0 {
            return Err(FinError::InvalidInput(
                format!("sigma must be > 0, got {sigma}"),
            ));
        }
        let m = (offset * (period as f64 - 1.0)).floor();
        let s = period as f64 / sigma;
        let mut raw_weights: Vec<f64> = (0..period)
            .map(|i| {
                let diff = i as f64 - m;
                (-diff * diff / (2.0 * s * s)).exp()
            })
            .collect();
        let sum: f64 = raw_weights.iter().sum();
        for w in &mut raw_weights {
            *w /= sum;
        }
        let weights: Vec<Decimal> = raw_weights
            .iter()
            .map(|&w| Decimal::try_from(w).unwrap_or(Decimal::ZERO))
            .collect();

        Ok(Self {
            name: name.into(),
            period,
            weights,
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Alma {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let val: Decimal = self
            .closes
            .iter()
            .zip(self.weights.iter())
            .map(|(&c, &w)| c * w)
            .sum();
        Ok(SignalValue::Scalar(val))
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
    fn test_alma_invalid_period() {
        assert!(Alma::new("a", 0, 0.85, 6.0).is_err());
    }

    #[test]
    fn test_alma_invalid_sigma() {
        assert!(Alma::new("a", 9, 0.85, 0.0).is_err());
        assert!(Alma::new("a", 9, 0.85, -1.0).is_err());
    }

    #[test]
    fn test_alma_unavailable_before_period() {
        let mut a = Alma::new("a", 5, 0.85, 6.0).unwrap();
        for _ in 0..4 {
            assert_eq!(a.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(matches!(a.update_bar(&bar("100")).unwrap(), SignalValue::Scalar(_)));
    }

    #[test]
    fn test_alma_flat_market_equals_price() {
        let mut a = Alma::new("a", 5, 0.85, 6.0).unwrap();
        for _ in 0..10 {
            a.update_bar(&bar("100")).unwrap();
        }
        match a.update_bar(&bar("100")).unwrap() {
            SignalValue::Scalar(v) => {
                let diff = (v - dec!(100)).abs();
                assert!(diff < dec!(0.001), "flat ALMA far from price: {v}");
            }
            _ => panic!("expected scalar"),
        }
    }

    #[test]
    fn test_alma_reset() {
        let mut a = Alma::new("a", 5, 0.85, 6.0).unwrap();
        for _ in 0..5 { a.update_bar(&bar("100")).unwrap(); }
        assert!(a.is_ready());
        a.reset();
        assert!(!a.is_ready());
    }

    #[test]
    fn test_alma_period_and_name() {
        let a = Alma::new("my_alma", 9, 0.85, 6.0).unwrap();
        assert_eq!(a.period(), 9);
        assert_eq!(a.name(), "my_alma");
    }
}
