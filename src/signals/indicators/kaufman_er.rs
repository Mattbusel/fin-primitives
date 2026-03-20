//! Kaufman Efficiency Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Kaufman Efficiency Ratio (ER) — measures how efficiently price moves in one direction.
///
/// ```text
/// ER = |close[n-1] − close[0]| / Σ|close[i] − close[i-1]|
/// ```
///
/// Range: 0 (completely noisy/choppy) to 1 (perfectly trending).
/// When all price movement is in one direction, ER = 1.
/// When price oscillates, ER approaches 0.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::KaufmanEr;
/// use fin_primitives::signals::Signal;
///
/// let er = KaufmanEr::new("er10", 10).unwrap();
/// assert_eq!(er.period(), 10);
/// ```
pub struct KaufmanEr {
    name: String,
    period: usize,
    history: VecDeque<Decimal>,
}

impl KaufmanEr {
    /// Constructs a new `KaufmanEr`.
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
            history: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for KaufmanEr {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.history.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.history.push_back(bar.close);
        if self.history.len() > self.period {
            self.history.pop_front();
        }
        if self.history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let first = *self.history.front().unwrap();
        let last = *self.history.back().unwrap();
        let direction = (last - first).abs();

        let volatility: Decimal = self
            .history
            .iter()
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (*w[1] - *w[0]).abs())
            .sum();

        if volatility.is_zero() {
            // Price is flat — perfectly "efficient" in the sense it went nowhere with no noise
            return Ok(SignalValue::Scalar(Decimal::ONE));
        }

        let er = direction
            .checked_div(volatility)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(er))
    }

    fn reset(&mut self) {
        self.history.clear();
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
    fn test_er_invalid_period() {
        assert!(KaufmanEr::new("er", 0).is_err());
        assert!(KaufmanEr::new("er", 1).is_err());
    }

    #[test]
    fn test_er_unavailable_before_period() {
        let mut er = KaufmanEr::new("er", 3).unwrap();
        assert_eq!(er.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(er.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!er.is_ready());
    }

    #[test]
    fn test_er_perfect_trend_equals_one() {
        // [100, 101, 102]: direction=2, volatility=2 → ER=1
        let mut er = KaufmanEr::new("er", 3).unwrap();
        er.update_bar(&bar("100")).unwrap();
        er.update_bar(&bar("101")).unwrap();
        let v = er.update_bar(&bar("102")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_er_oscillating_price_low_er() {
        // [100, 110, 100]: direction=0, volatility=20 → ER=0
        let mut er = KaufmanEr::new("er", 3).unwrap();
        er.update_bar(&bar("100")).unwrap();
        er.update_bar(&bar("110")).unwrap();
        let v = er.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_er_flat_returns_one() {
        let mut er = KaufmanEr::new("er", 3).unwrap();
        er.update_bar(&bar("100")).unwrap();
        er.update_bar(&bar("100")).unwrap();
        let v = er.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_er_reset() {
        let mut er = KaufmanEr::new("er", 2).unwrap();
        er.update_bar(&bar("100")).unwrap();
        er.update_bar(&bar("101")).unwrap();
        assert!(er.is_ready());
        er.reset();
        assert!(!er.is_ready());
    }
}
