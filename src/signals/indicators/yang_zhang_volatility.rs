//! Yang-Zhang Volatility estimator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Yang-Zhang historical volatility estimator (Yang & Zhang 2000).
///
/// The most efficient unbiased OHLC estimator. Combines the overnight return variance,
/// the open-to-close return variance, and the Rogers-Satchell drift-free variance.
///
/// Components:
/// - `σ²_o` = variance of overnight log returns `ln(O_t / C_{t-1})`
/// - `σ²_c` = variance of open-to-close log returns `ln(C_t / O_t)`
/// - `σ²_rs` = Rogers-Satchell estimator per bar
///
/// Combined: `σ² = σ²_o + k·σ²_c + (1−k)·σ²_rs`
/// where `k = 0.34 / (1.34 + (n+1)/(n−1))`
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars (first bar sets prev_close).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::YangZhangVolatility;
/// use fin_primitives::signals::Signal;
/// let yz = YangZhangVolatility::new("yz_20", 20).unwrap();
/// assert_eq!(yz.period(), 20);
/// ```
pub struct YangZhangVolatility {
    name: String,
    period: usize,
    prev_close: Option<f64>,
    overnight: VecDeque<f64>,
    open_close: VecDeque<f64>,
    rs: VecDeque<f64>,
}

impl YangZhangVolatility {
    /// Constructs a new `YangZhangVolatility` with the given name and period.
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
            prev_close: None,
            overnight: VecDeque::with_capacity(period),
            open_close: VecDeque::with_capacity(period),
            rs: VecDeque::with_capacity(period),
        })
    }

    fn variance(data: &VecDeque<f64>) -> f64 {
        let n = data.len() as f64;
        if n < 2.0 {
            return 0.0;
        }
        let mean = data.iter().sum::<f64>() / n;
        data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)
    }
}

impl Signal for YangZhangVolatility {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        use rust_decimal::prelude::ToPrimitive;
        let h = bar.high.to_f64().unwrap_or(0.0);
        let l = bar.low.to_f64().unwrap_or(0.0);
        let c = bar.close.to_f64().unwrap_or(0.0);
        let o = bar.open.to_f64().unwrap_or(0.0);
        if h <= 0.0 || l <= 0.0 || c <= 0.0 || o <= 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let Some(pc) = self.prev_close else {
            self.prev_close = Some(c);
            return Ok(SignalValue::Unavailable);
        };

        // Overnight return: ln(O / prev_C)
        if pc > 0.0 {
            self.overnight.push_back((o / pc).ln());
            if self.overnight.len() > self.period {
                self.overnight.pop_front();
            }
        }

        // Open-to-close return: ln(C / O)
        if o > 0.0 {
            self.open_close.push_back((c / o).ln());
            if self.open_close.len() > self.period {
                self.open_close.pop_front();
            }
        }

        // Rogers-Satchell per bar
        let rs_val = (h / c).ln() * (h / o).ln() + (l / c).ln() * (l / o).ln();
        self.rs.push_back(rs_val.max(0.0));
        if self.rs.len() > self.period {
            self.rs.pop_front();
        }

        self.prev_close = Some(c);

        if self.overnight.len() < self.period
            || self.open_close.len() < self.period
            || self.rs.len() < self.period
        {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let k = 0.34 / (1.34 + (n + 1.0) / (n - 1.0));
        let var_o = Self::variance(&self.overnight);
        let var_c = Self::variance(&self.open_close);
        let var_rs = self.rs.iter().sum::<f64>() / n;
        let var_yz = (var_o + k * var_c + (1.0 - k) * var_rs).max(0.0);
        let sigma = var_yz.sqrt();
        Decimal::try_from(sigma)
            .map(SignalValue::Scalar)
            .map_err(|_| FinError::ArithmeticOverflow)
    }

    fn is_ready(&self) -> bool {
        self.overnight.len() >= self.period
            && self.open_close.len() >= self.period
            && self.rs.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.overnight.clear();
        self.open_close.clear();
        self.rs.clear();
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
    fn test_period_too_small_fails() {
        assert!(matches!(
            YangZhangVolatility::new("yz", 1),
            Err(FinError::InvalidPeriod(1))
        ));
        assert!(matches!(
            YangZhangVolatility::new("yz", 0),
            Err(FinError::InvalidPeriod(0))
        ));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut yz = YangZhangVolatility::new("yz", 3).unwrap();
        for _ in 0..3 {
            let v = yz.update_bar(&bar("10", "12", "9", "11")).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ready_after_period_plus_one() {
        let mut yz = YangZhangVolatility::new("yz", 3).unwrap();
        // Need period+1 bars (first sets prev_close only)
        for _ in 0..4 {
            yz.update_bar(&bar("10", "12", "9", "11")).unwrap();
        }
        assert!(yz.is_ready());
    }

    #[test]
    fn test_sigma_non_negative() {
        let mut yz = YangZhangVolatility::new("yz", 3).unwrap();
        for _ in 0..5 {
            yz.update_bar(&bar("10", "12", "9", "11")).unwrap();
        }
        let v = yz.update_bar(&bar("10", "12", "9", "11")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s >= dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset_clears_state() {
        let mut yz = YangZhangVolatility::new("yz", 3).unwrap();
        for _ in 0..5 {
            yz.update_bar(&bar("10", "12", "9", "11")).unwrap();
        }
        assert!(yz.is_ready());
        yz.reset();
        assert!(!yz.is_ready());
        assert!(yz.prev_close.is_none());
    }

    #[test]
    fn test_wider_range_larger_vol() {
        let mut narrow = YangZhangVolatility::new("yz", 3).unwrap();
        let mut wide = YangZhangVolatility::new("yz", 3).unwrap();
        for _ in 0..5 {
            narrow.update_bar(&bar("100", "102", "98", "101")).unwrap();
            wide.update_bar(&bar("100", "120", "80", "101")).unwrap();
        }
        let nv = match narrow.update_bar(&bar("100", "102", "98", "101")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        let wv = match wide.update_bar(&bar("100", "120", "80", "101")).unwrap() {
            SignalValue::Scalar(v) => v,
            _ => panic!("expected scalar"),
        };
        assert!(wv > nv);
    }
}
