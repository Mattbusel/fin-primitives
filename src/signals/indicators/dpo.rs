//! Detrended Price Oscillator (DPO) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Detrended Price Oscillator (DPO).
///
/// Removes the trend from price by comparing the current close to a past SMA,
/// making it easier to identify price cycles.
///
/// ```text
/// shift  = period / 2 + 1
/// DPO[i] = close[i] - SMA(period)[i - shift]
/// ```
///
/// In practice the indicator buffers `period + shift` bars and then emits
/// `close[current - shift] - SMA(period)[current - shift - (period - 1)]`.
/// Because the SMA is centred in the past, DPO is a lagging oscillator.
///
/// Returns [`SignalValue::Unavailable`] until enough bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Dpo;
/// use fin_primitives::signals::Signal;
/// let dpo = Dpo::new("dpo_14", 14).unwrap();
/// assert_eq!(dpo.period(), 14);
/// ```
pub struct Dpo {
    name: String,
    period: usize,
    /// shift = period / 2 + 1
    shift: usize,
    /// Rolling window storing the last `period + shift` closes.
    window: VecDeque<Decimal>,
}

impl Dpo {
    /// Constructs a new `Dpo` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        let shift = period / 2 + 1;
        Ok(Self {
            name: name.into(),
            period,
            shift,
            window: VecDeque::with_capacity(period + shift),
        })
    }
}

impl Signal for Dpo {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        let needed = self.period + self.shift;
        if self.window.len() > needed {
            self.window.pop_front();
        }
        if self.window.len() < needed {
            return Ok(SignalValue::Unavailable);
        }

        // SMA is computed over the oldest `period` bars in the window.
        let sma_sum: Decimal = self.window.iter().take(self.period).sum();
        #[allow(clippy::cast_possible_truncation)]
        let sma = sma_sum / Decimal::from(self.period as u32);

        // The price used for comparison is `shift` bars ago = `window[period - 1]`.
        let compare_close = self.window[self.period - 1];

        Ok(SignalValue::Scalar(compare_close - sma))
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period + self.shift
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: Decimal) -> OhlcvBar {
        let p = Price::new(close).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p,
            high: p,
            low: p,
            close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_dpo_period_less_than_2_fails() {
        assert!(Dpo::new("d", 0).is_err());
        assert!(Dpo::new("d", 1).is_err());
    }

    #[test]
    fn test_dpo_period_accessor() {
        let d = Dpo::new("d4", 4).unwrap();
        assert_eq!(d.period(), 4);
    }

    #[test]
    fn test_dpo_unavailable_before_period() {
        // period=4, shift=3, needs 7 bars
        let mut d = Dpo::new("d4", 4).unwrap();
        for _ in 0..6 {
            assert_eq!(d.update_bar(&bar(dec!(100))).unwrap(), SignalValue::Unavailable);
        }
        assert!(!d.is_ready());
    }

    #[test]
    fn test_dpo_flat_series_is_zero() {
        // A flat price series: SMA == close everywhere, so DPO == 0
        let mut d = Dpo::new("d4", 4).unwrap();
        let needed = 4 + (4 / 2 + 1); // 7
        for _ in 0..needed {
            d.update_bar(&bar(dec!(100))).unwrap();
        }
        let result = d.update_bar(&bar(dec!(100))).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_dpo_reset() {
        let mut d = Dpo::new("d4", 4).unwrap();
        let needed = 4 + (4 / 2 + 1);
        for _ in 0..needed {
            d.update_bar(&bar(dec!(100))).unwrap();
        }
        assert!(d.is_ready());
        d.reset();
        assert!(!d.is_ready());
        assert_eq!(d.update_bar(&bar(dec!(100))).unwrap(), SignalValue::Unavailable);
    }
}
