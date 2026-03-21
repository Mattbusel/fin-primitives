//! Weighted Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Weighted Momentum.
///
/// A time-weighted version of momentum where more recent returns receive
/// linearly higher weights. This emphasizes recent price action while still
/// considering the full lookback period.
///
/// Formula: `wm = Σ(w_i * r_i) / Σ(w_i)` where `w_i = i + 1` (1-indexed from oldest)
///
/// The returns are `r_i = close_i - close_{i-1}` over `period` consecutive bars.
///
/// Returns `SignalValue::Unavailable` until `period + 1` closes accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WeightedMomentum;
/// use fin_primitives::signals::Signal;
/// let wm = WeightedMomentum::new("wm_14", 14).unwrap();
/// assert_eq!(wm.period(), 14);
/// ```
pub struct WeightedMomentum {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
}

impl WeightedMomentum {
    /// Constructs a new `WeightedMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            closes: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for WeightedMomentum {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let mut weighted_sum = Decimal::ZERO;
        let mut weight_total = Decimal::ZERO;

        // Compute weighted sum of returns; oldest return gets weight 1, newest gets weight `period`
        for i in 0..self.period {
            let ret = self.closes[i + 1] - self.closes[i];
            #[allow(clippy::cast_possible_truncation)]
            let weight = Decimal::from((i + 1) as u32);
            weighted_sum += ret.checked_mul(weight).ok_or(FinError::ArithmeticOverflow)?;
            weight_total += weight;
        }

        let wm = weighted_sum.checked_div(weight_total).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(wm))
    }

    fn is_ready(&self) -> bool {
        self.closes.len() >= self.period + 1
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

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_period_zero_fails() {
        assert!(matches!(WeightedMomentum::new("wm", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut wm = WeightedMomentum::new("wm", 3).unwrap();
        assert_eq!(wm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_flat_price_zero_momentum() {
        let mut wm = WeightedMomentum::new("wm", 3).unwrap();
        for _ in 0..4 {
            wm.update_bar(&bar("100")).unwrap();
        }
        let v = wm.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_constant_rise_positive() {
        let mut wm = WeightedMomentum::new("wm", 3).unwrap();
        // Prices: 100, 102, 104, 106 → returns all +2, weighted avg = 2
        wm.update_bar(&bar("100")).unwrap();
        wm.update_bar(&bar("102")).unwrap();
        wm.update_bar(&bar("104")).unwrap();
        let v = wm.update_bar(&bar("106")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_reset() {
        let mut wm = WeightedMomentum::new("wm", 2).unwrap();
        wm.update_bar(&bar("100")).unwrap();
        wm.update_bar(&bar("101")).unwrap();
        wm.update_bar(&bar("102")).unwrap();
        assert!(wm.is_ready());
        wm.reset();
        assert!(!wm.is_ready());
    }
}
