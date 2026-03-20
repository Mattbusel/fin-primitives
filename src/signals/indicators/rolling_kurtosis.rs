//! Rolling Kurtosis — excess kurtosis of close-to-close returns over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling Kurtosis — excess kurtosis of close-to-close returns over the last `period` bars.
///
/// Excess kurtosis = `E[(X - μ)⁴] / σ⁴ - 3`:
/// - **> 0 (leptokurtic)**: heavier tails than normal — fat-tail risk, spike returns.
/// - **= 0**: normal distribution tails.
/// - **< 0 (platykurtic)**: lighter tails — returns clustered near mean.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen,
/// or when variance is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RollingKurtosis;
/// use fin_primitives::signals::Signal;
/// let rk = RollingKurtosis::new("kurt_20", 20).unwrap();
/// assert_eq!(rk.period(), 20);
/// ```
pub struct RollingKurtosis {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl RollingKurtosis {
    /// Constructs a new `RollingKurtosis`.
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
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for RollingKurtosis {
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
        if n < 2.0 {
            return Ok(SignalValue::Unavailable);
        }

        let mean = returns.iter().sum::<f64>() / n;
        let variance = returns.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / n;

        if variance == 0.0 {
            return Ok(SignalValue::Unavailable);
        }

        let fourth_moment = returns
            .iter()
            .map(|r| {
                let d = r - mean;
                d * d * d * d
            })
            .sum::<f64>()
            / n;

        let excess_kurtosis = (fourth_moment / (variance * variance)) - 3.0;

        Decimal::try_from(excess_kurtosis)
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
    fn test_rk_invalid_period() {
        assert!(RollingKurtosis::new("kurt", 0).is_err());
        assert!(RollingKurtosis::new("kurt", 3).is_err());
    }

    #[test]
    fn test_rk_unavailable_before_warm_up() {
        let mut s = RollingKurtosis::new("kurt", 4).unwrap();
        for _ in 0..4 {
            assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_rk_flat_prices_unavailable() {
        let mut s = RollingKurtosis::new("kurt", 4).unwrap();
        for _ in 0..5 { s.update_bar(&bar("100")).unwrap(); }
        let v = s.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rk_spike_gives_high_kurtosis() {
        let mut s = RollingKurtosis::new("kurt", 5).unwrap();
        // Many small returns, then one spike
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("100.1")).unwrap();
        s.update_bar(&bar("100.2")).unwrap();
        s.update_bar(&bar("100.3")).unwrap();
        s.update_bar(&bar("100.4")).unwrap();
        let v = s.update_bar(&bar("110")).unwrap(); // big spike
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(0), "spike should give positive excess kurtosis: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rk_reset() {
        let mut s = RollingKurtosis::new("kurt", 4).unwrap();
        for p in &["100","101","102","103","104"] {
            s.update_bar(&bar(p)).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
