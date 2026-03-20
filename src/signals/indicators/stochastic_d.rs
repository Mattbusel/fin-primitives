//! Stochastic %D oscillator — the smoothed signal line for %K.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use crate::signals::indicators::StochasticK;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Stochastic %D oscillator: a `d_period`-bar SMA of `%K`.
///
/// The classic setting is `k_period = 14`, `d_period = 3`.
///
/// ```text
/// %D = SMA(%K, d_period)
/// ```
///
/// Returns `SignalValue::Unavailable` until `k_period + d_period - 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::StochasticD;
/// use fin_primitives::signals::Signal;
///
/// let stoch = StochasticD::new("stoch_d", 14, 3).unwrap();
/// assert_eq!(stoch.period(), 14);
/// ```
pub struct StochasticD {
    name: String,
    k_period: usize,
    d_period: usize,
    stoch_k: StochasticK,
    k_values: VecDeque<Decimal>,
}

impl StochasticD {
    /// Constructs a new `StochasticD`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `k_period == 0` or `d_period == 0`.
    pub fn new(
        name: impl Into<String>,
        k_period: usize,
        d_period: usize,
    ) -> Result<Self, FinError> {
        if d_period == 0 {
            return Err(FinError::InvalidPeriod(d_period));
        }
        Ok(Self {
            name: name.into(),
            k_period,
            d_period,
            stoch_k: StochasticK::new("_k", k_period)?,
            k_values: VecDeque::with_capacity(d_period),
        })
    }

    /// Returns the `%D` smoothing period.
    pub fn d_period(&self) -> usize {
        self.d_period
    }
}

impl Signal for StochasticD {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let k_val = self.stoch_k.update(bar)?;
        let k = match k_val {
            SignalValue::Scalar(v) => v,
            SignalValue::Unavailable => return Ok(SignalValue::Unavailable),
        };

        self.k_values.push_back(k);
        if self.k_values.len() > self.d_period {
            self.k_values.pop_front();
        }
        if self.k_values.len() < self.d_period {
            return Ok(SignalValue::Unavailable);
        }

        #[allow(clippy::cast_possible_truncation)]
        let sum: Decimal = self.k_values.iter().copied().sum();
        let d = sum / Decimal::from(self.d_period as u32);
        Ok(SignalValue::Scalar(d))
    }

    fn is_ready(&self) -> bool {
        self.k_values.len() >= self.d_period && self.stoch_k.is_ready()
    }

    fn period(&self) -> usize {
        self.k_period
    }

    fn reset(&mut self) {
        self.stoch_k.reset();
        self.k_values.clear();
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

    fn close_bar(c: &str) -> OhlcvBar {
        bar(c, c, c, c)
    }

    #[test]
    fn test_stochastic_d_period_0_fails() {
        assert!(StochasticD::new("d", 3, 0).is_err());
        assert!(StochasticD::new("d", 0, 3).is_err());
    }

    #[test]
    fn test_stochastic_d_unavailable_before_warmup() {
        // k_period=3, d_period=3: need 3+3-1=5 bars total
        let mut d = StochasticD::new("d", 3, 3).unwrap();
        for _ in 0..4 {
            assert_eq!(d.update_bar(&close_bar("100")).unwrap(), SignalValue::Unavailable);
        }
        assert!(!d.is_ready());
    }

    #[test]
    fn test_stochastic_d_flat_price_returns_50() {
        // Flat price: %K = 50, %D = SMA(50,50,50) = 50
        let mut d = StochasticD::new("d", 3, 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..10 {
            last = d.update_bar(&close_bar("100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_stochastic_d_reset() {
        let mut d = StochasticD::new("d", 3, 3).unwrap();
        for _ in 0..10 {
            d.update_bar(&close_bar("100")).unwrap();
        }
        assert!(d.is_ready());
        d.reset();
        assert!(!d.is_ready());
        assert_eq!(d.update_bar(&close_bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_stochastic_d_period_accessors() {
        let d = StochasticD::new("d14_3", 14, 3).unwrap();
        assert_eq!(d.period(), 14);
        assert_eq!(d.d_period(), 3);
    }
}
