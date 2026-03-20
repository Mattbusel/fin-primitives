//! Smoothed Moving Average (SMMA / Wilder MA).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Smoothed Moving Average (also called Wilder Moving Average).
///
/// Uses `alpha = 1 / period`, giving a slower decay than EMA's `2 / (period+1)`.
/// Seeded with an SMA over the first `period` bars.
///
/// `SMMA[i] = alpha * close[i] + (1 - alpha) * SMMA[i-1]`
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Smma;
/// use fin_primitives::signals::Signal;
///
/// let s = Smma::new("smma14", 14).unwrap();
/// assert_eq!(s.period(), 14);
/// assert!(!s.is_ready());
/// ```
pub struct Smma {
    name: String,
    period: usize,
    alpha: Decimal,
    seed_buf: VecDeque<Decimal>,
    smma: Option<Decimal>,
}

impl Smma {
    /// Constructs a new `Smma`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        #[allow(clippy::cast_possible_truncation)]
        let alpha = Decimal::ONE / Decimal::from(period as u32);
        Ok(Self {
            name: name.into(),
            period,
            alpha,
            seed_buf: VecDeque::with_capacity(period),
            smma: None,
        })
    }
}

impl Signal for Smma {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if self.smma.is_none() {
            self.seed_buf.push_back(bar.close);
            if self.seed_buf.len() < self.period {
                return Ok(SignalValue::Unavailable);
            }
            // Seed SMMA with SMA of first `period` bars
            let seed: Decimal = self.seed_buf.iter().copied().sum::<Decimal>()
                / Decimal::from(self.period as u32);
            self.smma = Some(seed);
            return Ok(SignalValue::Scalar(seed));
        }
        let prev = self.smma.unwrap();
        let new_smma = self.alpha * bar.close + (Decimal::ONE - self.alpha) * prev;
        self.smma = Some(new_smma);
        Ok(SignalValue::Scalar(new_smma))
    }

    fn is_ready(&self) -> bool {
        self.smma.is_some()
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.seed_buf.clear();
        self.smma = None;
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
    fn test_smma_invalid_period() {
        assert!(Smma::new("s", 0).is_err());
    }

    #[test]
    fn test_smma_unavailable_before_period() {
        let mut s = Smma::new("s", 3).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(matches!(s.update_bar(&bar("100")).unwrap(), SignalValue::Scalar(_)));
    }

    #[test]
    fn test_smma_flat_market_equals_price() {
        let mut s = Smma::new("s", 5).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..20 {
            last = s.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            let diff = (v - dec!(100)).abs();
            assert!(diff < dec!(0.001), "SMMA far from flat price: {v}");
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_smma_seed_equals_sma() {
        // After exactly `period` bars all at the same price, SMMA = that price
        let mut s = Smma::new("s", 4).unwrap();
        for _ in 0..3 { s.update_bar(&bar("50")).unwrap(); }
        let seed = s.update_bar(&bar("50")).unwrap();
        assert_eq!(seed, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_smma_reset() {
        let mut s = Smma::new("s", 3).unwrap();
        for _ in 0..5 { s.update_bar(&bar("100")).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }

    #[test]
    fn test_smma_period_and_name() {
        let s = Smma::new("my_smma", 14).unwrap();
        assert_eq!(s.period(), 14);
        assert_eq!(s.name(), "my_smma");
    }
}
