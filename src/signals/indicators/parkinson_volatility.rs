//! Parkinson's Volatility estimator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Parkinson's historical volatility estimator (Parkinson 1980).
///
/// Uses the intraday high-low range to estimate realized volatility. More efficient
/// than close-to-close estimators as it incorporates intraday price movement.
///
/// Formula: `σ = sqrt( (1 / (4 · n · ln 2)) · Σᵢ (ln(Hᵢ / Lᵢ))² )`
///
/// Returns `SignalValue::Unavailable` until `period` bars with non-zero ranges have
/// been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ParkinsonVolatility;
/// use fin_primitives::signals::Signal;
/// let pv = ParkinsonVolatility::new("pv_20", 20).unwrap();
/// assert_eq!(pv.period(), 20);
/// ```
pub struct ParkinsonVolatility {
    name: String,
    period: usize,
    log_hl_sq: VecDeque<f64>,
}

impl ParkinsonVolatility {
    /// Constructs a new `ParkinsonVolatility` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, log_hl_sq: VecDeque::with_capacity(period) })
    }
}

impl Signal for ParkinsonVolatility {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;
        let h = bar.high.to_f64().unwrap_or(0.0);
        let l = bar.low.to_f64().unwrap_or(0.0);
        if h <= 0.0 || l <= 0.0 || l >= h {
            return Ok(SignalValue::Unavailable);
        }
        let ln_hl = (h / l).ln();
        self.log_hl_sq.push_back(ln_hl * ln_hl);
        if self.log_hl_sq.len() > self.period {
            self.log_hl_sq.pop_front();
        }
        if self.log_hl_sq.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let sum: f64 = self.log_hl_sq.iter().sum();
        let sigma = (sum / (4.0 * self.period as f64 * std::f64::consts::LN_2)).sqrt();
        Decimal::try_from(sigma)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn is_ready(&self) -> bool {
        self.log_hl_sq.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.log_hl_sq.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(
            ParkinsonVolatility::new("pv", 0),
            Err(FinError::InvalidPeriod(0))
        ));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut pv = ParkinsonVolatility::new("pv", 3).unwrap();
        let v = pv.update_bar(&bar("10", "12", "9", "11")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
        assert!(!pv.is_ready());
    }

    #[test]
    fn test_ready_after_period() {
        let mut pv = ParkinsonVolatility::new("pv", 2).unwrap();
        pv.update_bar(&bar("10", "12", "9", "11")).unwrap();
        let v = pv.update_bar(&bar("11", "13", "10", "12")).unwrap();
        assert!(pv.is_ready());
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_sigma_positive_for_range_bars() {
        let mut pv = ParkinsonVolatility::new("pv", 5).unwrap();
        for _ in 0..5 {
            pv.update_bar(&bar("10", "11", "9", "10")).unwrap();
        }
        let v = pv.update_bar(&bar("10", "11", "9", "10")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_flat_bar_skipped() {
        let mut pv = ParkinsonVolatility::new("pv", 1).unwrap();
        let v = pv.update_bar(&bar("10", "10", "10", "10")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_reset_clears_state() {
        let mut pv = ParkinsonVolatility::new("pv", 2).unwrap();
        pv.update_bar(&bar("10", "12", "9", "11")).unwrap();
        pv.update_bar(&bar("11", "13", "10", "12")).unwrap();
        assert!(pv.is_ready());
        pv.reset();
        assert!(!pv.is_ready());
        let v = pv.update_bar(&bar("10", "12", "9", "11")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rolls_window() {
        let mut pv = ParkinsonVolatility::new("pv", 3).unwrap();
        for _ in 0..5 {
            pv.update_bar(&bar("10", "12", "9", "11")).unwrap();
        }
        assert!(pv.is_ready());
    }

    #[test]
    fn test_wider_range_gives_higher_vol() {
        let mut pv_narrow = ParkinsonVolatility::new("pv", 3).unwrap();
        let mut pv_wide = ParkinsonVolatility::new("pv", 3).unwrap();
        for _ in 0..3 {
            pv_narrow.update_bar(&bar("100", "101", "99", "100")).unwrap();
            pv_wide.update_bar(&bar("100", "110", "90", "100")).unwrap();
        }
        let narrow_val = match pv_narrow.update_bar(&bar("100", "101", "99", "100")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        let wide_val = match pv_wide.update_bar(&bar("100", "110", "90", "100")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        assert!(wide_val > narrow_val);
    }
}
