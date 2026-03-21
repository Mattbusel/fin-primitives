//! On-Balance Volume Moving Average indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// On-Balance Volume Moving Average (OBV-MA).
///
/// Computes a simple moving average of the OBV series over a rolling window.
/// This smooths the raw OBV to reduce noise and make trend signals cleaner.
///
/// OBV calculation:
/// - If `close > prev_close`: `obv += volume`
/// - If `close < prev_close`: `obv -= volume`
/// - If `close == prev_close`: `obv unchanged`
///
/// OBV-MA: `mean(obv, period)` over the last `period` OBV values.
///
/// Returns `SignalValue::Unavailable` until `period` OBV values accumulated
/// (requires `period + 1` total bars).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OnBalanceVolumeMA;
/// use fin_primitives::signals::Signal;
/// let obvma = OnBalanceVolumeMA::new("obvma_20", 20).unwrap();
/// assert_eq!(obvma.period(), 20);
/// ```
pub struct OnBalanceVolumeMA {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    obv: Decimal,
    obv_history: VecDeque<Decimal>,
}

impl OnBalanceVolumeMA {
    /// Constructs a new `OnBalanceVolumeMA`.
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
            prev_close: None,
            obv: Decimal::ZERO,
            obv_history: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for OnBalanceVolumeMA {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(prev_c) = self.prev_close {
            if bar.close > prev_c {
                self.obv += bar.volume;
            } else if bar.close < prev_c {
                self.obv -= bar.volume;
            }
            // equal close: no change to OBV

            self.obv_history.push_back(self.obv);
            if self.obv_history.len() > self.period {
                self.obv_history.pop_front();
            }
        }

        self.prev_close = Some(bar.close);

        if self.obv_history.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.obv_history.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.obv_history.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.obv = Decimal::ZERO;
        self.obv_history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str, vol: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(OnBalanceVolumeMA::new("obvma", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut obvma = OnBalanceVolumeMA::new("obvma", 3).unwrap();
        assert_eq!(obvma.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rising_prices_positive_obv() {
        let mut obvma = OnBalanceVolumeMA::new("obvma", 3).unwrap();
        obvma.update_bar(&bar("100", "1000")).unwrap();
        obvma.update_bar(&bar("101", "1000")).unwrap(); // obv=1000
        obvma.update_bar(&bar("102", "1000")).unwrap(); // obv=2000
        obvma.update_bar(&bar("103", "1000")).unwrap(); // obv=3000
        // obvma history: [1000, 2000, 3000], avg=2000
        let v = obvma.update_bar(&bar("104", "1000")).unwrap(); // obv=4000
        // history: [2000,3000,4000], avg=3000
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut obvma = OnBalanceVolumeMA::new("obvma", 2).unwrap();
        for _ in 0..3 {
            obvma.update_bar(&bar("100", "1000")).unwrap();
        }
        assert!(obvma.is_ready());
        obvma.reset();
        assert!(!obvma.is_ready());
    }
}
