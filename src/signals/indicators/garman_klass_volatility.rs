//! Garman-Klass Volatility estimator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Garman-Klass historical volatility estimator (Garman & Klass 1980).
///
/// Extends Parkinson's estimator by incorporating open-to-close drift, producing a
/// more efficient volatility estimate from OHLC data.
///
/// Formula per bar: `gk = 0.5 · (ln(H/L))² − (2·ln2 − 1) · (ln(C/O))²`
///
/// Aggregate: `σ = sqrt( (1/n) · Σ gk_i )`
///
/// Returns `SignalValue::Unavailable` until `period` valid OHLC bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::GarmanKlassVolatility;
/// use fin_primitives::signals::Signal;
/// let gk = GarmanKlassVolatility::new("gk_20", 20).unwrap();
/// assert_eq!(gk.period(), 20);
/// ```
pub struct GarmanKlassVolatility {
    name: String,
    period: usize,
    gk_values: VecDeque<f64>,
}

impl GarmanKlassVolatility {
    /// Constructs a new `GarmanKlassVolatility` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, gk_values: VecDeque::with_capacity(period) })
    }
}

impl Signal for GarmanKlassVolatility {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;
        let h = bar.high.to_f64().unwrap_or(0.0);
        let l = bar.low.to_f64().unwrap_or(0.0);
        let c = bar.close.to_f64().unwrap_or(0.0);
        let o = bar.open.to_f64().unwrap_or(0.0);
        if h <= 0.0 || l <= 0.0 || c <= 0.0 || o <= 0.0 || l >= h {
            return Ok(SignalValue::Unavailable);
        }
        let ln_hl = (h / l).ln();
        let ln_co = (c / o).ln();
        let gk = 0.5 * ln_hl * ln_hl - (2.0 * std::f64::consts::LN_2 - 1.0) * ln_co * ln_co;
        self.gk_values.push_back(gk);
        if self.gk_values.len() > self.period {
            self.gk_values.pop_front();
        }
        if self.gk_values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let mean = self.gk_values.iter().sum::<f64>() / self.period as f64;
        let sigma = mean.max(0.0).sqrt();
        Decimal::try_from(sigma)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn is_ready(&self) -> bool {
        self.gk_values.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.gk_values.clear();
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
            GarmanKlassVolatility::new("gk", 0),
            Err(FinError::InvalidPeriod(0))
        ));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut gk = GarmanKlassVolatility::new("gk", 3).unwrap();
        let v = gk.update_bar(&bar("10", "12", "9", "11")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_ready_after_period() {
        let mut gk = GarmanKlassVolatility::new("gk", 2).unwrap();
        gk.update_bar(&bar("10", "12", "9", "11")).unwrap();
        let v = gk.update_bar(&bar("11", "13", "10", "12")).unwrap();
        assert!(gk.is_ready());
        assert!(matches!(v, SignalValue::Scalar(_)));
    }

    #[test]
    fn test_flat_bar_skipped() {
        let mut gk = GarmanKlassVolatility::new("gk", 1).unwrap();
        let v = gk.update_bar(&bar("10", "10", "10", "10")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_sigma_non_negative() {
        let mut gk = GarmanKlassVolatility::new("gk", 5).unwrap();
        for _ in 0..5 {
            gk.update_bar(&bar("10", "12", "9", "10")).unwrap();
        }
        let v = gk.update_bar(&bar("10", "12", "9", "10")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s >= dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset_clears_state() {
        let mut gk = GarmanKlassVolatility::new("gk", 2).unwrap();
        gk.update_bar(&bar("10", "12", "9", "11")).unwrap();
        gk.update_bar(&bar("11", "13", "10", "12")).unwrap();
        assert!(gk.is_ready());
        gk.reset();
        assert!(!gk.is_ready());
    }

    #[test]
    fn test_wider_range_larger_vol() {
        let mut narrow = GarmanKlassVolatility::new("gk", 3).unwrap();
        let mut wide = GarmanKlassVolatility::new("gk", 3).unwrap();
        for _ in 0..3 {
            narrow.update_bar(&bar("100", "101", "99", "100")).unwrap();
            wide.update_bar(&bar("100", "115", "85", "100")).unwrap();
        }
        let nv = match narrow.update_bar(&bar("100", "101", "99", "100")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        let wv = match wide.update_bar(&bar("100", "115", "85", "100")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        assert!(wv > nv);
    }
}
