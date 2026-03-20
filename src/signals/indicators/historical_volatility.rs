//! Historical (Realized) Volatility indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Annualized historical (realized) volatility as a percentage.
///
/// ```text
/// log_return[i] = ln(close[i] / close[i-1])
/// HV = StdDev(log_returns, period) × sqrt(annualization_factor) × 100
/// ```
///
/// The default annualization factor is 252 (trading days per year).
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HistoricalVolatility;
/// use fin_primitives::signals::Signal;
/// let hv = HistoricalVolatility::new("hv20", 20, 252).unwrap();
/// assert_eq!(hv.period(), 20);
/// ```
pub struct HistoricalVolatility {
    name: String,
    period: usize,
    annualization: f64,
    closes: VecDeque<f64>,
}

impl HistoricalVolatility {
    /// Constructs a new `HistoricalVolatility`.
    ///
    /// - `period`: number of log-returns (requires `period + 1` close prices).
    /// - `annualization`: number of periods per year (252 for daily, 52 for weekly).
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize, annualization: u32) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            annualization: annualization as f64,
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for HistoricalVolatility {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;
        let c = bar.close.to_f64().unwrap_or(0.0);
        self.closes.push_back(c);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        // Compute log returns
        let mut log_rets = Vec::with_capacity(self.period);
        for i in 1..self.closes.len() {
            let prev = self.closes[i - 1];
            let curr = self.closes[i];
            if prev <= 0.0 { return Ok(SignalValue::Unavailable); }
            log_rets.push((curr / prev).ln());
        }

        let n = log_rets.len() as f64;
        let mean = log_rets.iter().sum::<f64>() / n;
        let variance = log_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
        let hv = variance.sqrt() * self.annualization.sqrt() * 100.0;

        Ok(SignalValue::Scalar(
            Decimal::try_from(hv).unwrap_or(Decimal::ZERO),
        ))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period + 1 }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
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
    fn test_hv_zero_period_fails() {
        assert!(HistoricalVolatility::new("hv", 0, 252).is_err());
    }

    #[test]
    fn test_hv_unavailable_before_warmup() {
        let mut hv = HistoricalVolatility::new("hv3", 3, 252).unwrap();
        assert_eq!(hv.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(hv.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert_eq!(hv.update_bar(&bar("102")).unwrap(), SignalValue::Unavailable);
        assert!(!hv.is_ready());
    }

    #[test]
    fn test_hv_constant_prices_zero_vol() {
        let mut hv = HistoricalVolatility::new("hv3", 3, 252).unwrap();
        for _ in 0..5 {
            hv.update_bar(&bar("100")).unwrap();
        }
        let v = hv.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert_eq!(val, dec!(0), "constant prices → zero volatility");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hv_positive_for_volatile_prices() {
        let mut hv = HistoricalVolatility::new("hv3", 3, 252).unwrap();
        let prices = ["100", "102", "99", "103", "97"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = hv.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(val) = last {
            assert!(val > dec!(0), "volatile prices → positive HV: {val}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hv_reset() {
        let mut hv = HistoricalVolatility::new("hv3", 3, 252).unwrap();
        for p in &["100", "102", "99", "103"] {
            hv.update_bar(&bar(p)).unwrap();
        }
        assert!(hv.is_ready());
        hv.reset();
        assert!(!hv.is_ready());
    }
}
