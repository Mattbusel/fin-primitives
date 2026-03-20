//! Return Over Volatility — rolling mean return divided by rolling std dev of returns.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Return Over Volatility — `mean(returns) / std_dev(returns)` over N bars.
///
/// A Sharpe-ratio-like measure on price returns over a rolling window:
/// - **Positive**: upward trend with positive mean return.
/// - **Negative**: downward trend.
/// - **Near 0**: noisy / mean-reverting.
/// - **Large magnitude**: strong trend relative to volatility.
///
/// Uses close-to-close returns and population standard deviation.
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars are seen,
/// or when std dev of returns is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ReturnOverVolatility;
/// use fin_primitives::signals::Signal;
/// let rov = ReturnOverVolatility::new("rov_20", 20).unwrap();
/// assert_eq!(rov.period(), 20);
/// ```
pub struct ReturnOverVolatility {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl ReturnOverVolatility {
    /// Constructs a new `ReturnOverVolatility`.
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
}

impl Signal for ReturnOverVolatility {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.closes.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        let returns: Vec<f64> = self
            .closes
            .iter()
            .collect::<Vec<_>>()
            .windows(2)
            .filter_map(|w| {
                let prev = w[0].to_f64()?;
                let curr = w[1].to_f64()?;
                if prev == 0.0 { None } else { Some((curr - prev) / prev) }
            })
            .collect();

        let n = returns.len() as f64;
        if n < 1.0 {
            return Ok(SignalValue::Unavailable);
        }

        let mean = returns.iter().sum::<f64>() / n;
        let variance = returns.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        if std_dev == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let rov = mean / std_dev;
        Decimal::try_from(rov)
            .map(SignalValue::Scalar)
            .or(Ok(SignalValue::Unavailable))
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
    fn test_rov_invalid_period() {
        assert!(ReturnOverVolatility::new("rov", 0).is_err());
        assert!(ReturnOverVolatility::new("rov", 1).is_err());
    }

    #[test]
    fn test_rov_unavailable_before_warm_up() {
        let mut s = ReturnOverVolatility::new("rov", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_rov_constant_prices_unavailable() {
        let mut s = ReturnOverVolatility::new("rov", 3).unwrap();
        for _ in 0..4 {
            s.update_bar(&bar("100")).unwrap();
        }
        // All returns = 0 → std_dev = 0 → Unavailable
        let v = s.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rov_trending_series_positive() {
        let mut s = ReturnOverVolatility::new("rov", 4).unwrap();
        let prices = ["100", "102", "105", "107", "110"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = s.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(r) = last {
            assert!(r > dec!(0), "uptrend should give positive RoV: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rov_reset() {
        let mut s = ReturnOverVolatility::new("rov", 3).unwrap();
        for p in &["100", "102", "105", "107"] {
            s.update_bar(&bar(p)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
