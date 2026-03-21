//! Rolling Return Kurtosis indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Return Kurtosis — the excess kurtosis of close-to-close returns over the
/// last `period` bars.
///
/// ```text
/// ret[i]   = (close[i] - close[i-1]) / close[i-1]
/// kurt     = (mean((ret - mean)^4)) / std^4   −   3
/// ```
///
/// Excess kurtosis subtracts 3 so that a normal distribution scores 0:
/// - **> 0 (leptokurtic)**: fat tails — extreme returns are more likely than normal.
/// - **< 0 (platykurtic)**: thin tails — returns cluster close to the mean.
/// - **≈ 0**: return distribution is approximately normal.
///
/// Fat tails are common in financial returns and indicate tail risk.
/// Note: distinct from `RollingKurtosis` which operates on raw closes.
///
/// Returns [`SignalValue::Unavailable`] until `period` returns are collected
/// (`period + 1` closes), or when the standard deviation is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period < 4`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ReturnKurtosis;
/// use fin_primitives::signals::Signal;
/// let rk = ReturnKurtosis::new("rk_20", 20).unwrap();
/// assert_eq!(rk.period(), 20);
/// ```
pub struct ReturnKurtosis {
    name: String,
    period: usize,
    returns: VecDeque<f64>,
    prev_close: Option<f64>,
}

impl ReturnKurtosis {
    /// Constructs a new `ReturnKurtosis`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 4`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 4 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            returns: VecDeque::with_capacity(period),
            prev_close: None,
        })
    }
}

impl Signal for ReturnKurtosis {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.returns.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;

        let c = bar.close.to_f64().unwrap_or(0.0);
        if let Some(pc) = self.prev_close {
            if pc > 0.0 {
                let ret = (c - pc) / pc;
                self.returns.push_back(ret);
                if self.returns.len() > self.period {
                    self.returns.pop_front();
                }
            }
        }
        self.prev_close = Some(c);

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.returns.len() as f64;
        let mean = self.returns.iter().sum::<f64>() / n;
        let variance = self.returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let kurt = self.returns
            .iter()
            .map(|r| ((r - mean) / std_dev).powi(4))
            .sum::<f64>()
            / n
            - 3.0; // excess kurtosis

        Decimal::try_from(kurt)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn reset(&mut self) {
        self.returns.clear();
        self.prev_close = None;
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
    fn test_rk_invalid_period() {
        assert!(ReturnKurtosis::new("rk", 0).is_err());
        assert!(ReturnKurtosis::new("rk", 3).is_err());
    }

    #[test]
    fn test_rk_unavailable_during_warmup() {
        let mut rk = ReturnKurtosis::new("rk", 5).unwrap();
        for p in &["100", "101", "99", "102", "100"] {
            assert_eq!(rk.update_bar(&bar(p)).unwrap(), SignalValue::Unavailable);
        }
        assert!(!rk.is_ready());
    }

    #[test]
    fn test_rk_flat_prices_unavailable() {
        let mut rk = ReturnKurtosis::new("rk", 4).unwrap();
        for _ in 0..7 { rk.update_bar(&bar("100")).unwrap(); }
        assert_eq!(rk.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rk_fat_tails_positive() {
        // Mostly zero returns with one extreme value → positive (fat-tail) kurtosis
        let mut rk = ReturnKurtosis::new("rk", 5).unwrap();
        rk.update_bar(&bar("100")).unwrap();
        rk.update_bar(&bar("100.01")).unwrap(); // ~0.01%
        rk.update_bar(&bar("100.02")).unwrap(); // ~0.01%
        rk.update_bar(&bar("100.03")).unwrap(); // ~0.01%
        rk.update_bar(&bar("100.04")).unwrap(); // ~0.01%
        if let SignalValue::Scalar(v) = rk.update_bar(&bar("120")).unwrap() { // big +~20%
            assert!(v > dec!(0), "outlier return → positive excess kurtosis: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rk_reset() {
        let mut rk = ReturnKurtosis::new("rk", 4).unwrap();
        for p in &["100","101","99","102","100"] { rk.update_bar(&bar(p)).unwrap(); }
        assert!(rk.is_ready());
        rk.reset();
        assert!(!rk.is_ready());
    }
}
