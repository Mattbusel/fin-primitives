//! Stochastic RSI indicator.
//!
//! Applies the Stochastic %K formula to RSI values over a rolling window,
//! producing a momentum oscillator that is more sensitive than plain RSI.

use crate::error::FinError;
use crate::signals::indicators::Rsi;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Stochastic RSI — Stochastic oscillator applied to RSI values.
///
/// ```text
/// StochRSI = (RSI - min(RSI, stoch_period)) / (max(RSI, stoch_period) - min(RSI, stoch_period)) × 100
/// ```
///
/// Values near 100 indicate RSI is at a recent extreme high (overbought momentum);
/// values near 0 indicate RSI is at a recent extreme low (oversold momentum).
/// Returns `50` when all RSI values in the window are equal (zero range).
///
/// Returns [`SignalValue::Unavailable`] until `rsi_period + stoch_period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::StochRsi;
/// use fin_primitives::signals::Signal;
///
/// let sr = StochRsi::new("srsi14_3", 14, 3).unwrap();
/// assert_eq!(sr.period(), 14);
/// ```
pub struct StochRsi {
    name: String,
    rsi_period: usize,
    stoch_period: usize,
    rsi: Rsi,
    rsi_window: VecDeque<Decimal>,
}

impl StochRsi {
    /// Constructs a new `StochRsi`.
    ///
    /// - `rsi_period`: lookback for the inner RSI (e.g. 14)
    /// - `stoch_period`: rolling window applied to RSI values (e.g. 3 or 14)
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is 0.
    pub fn new(
        name: impl Into<String>,
        rsi_period: usize,
        stoch_period: usize,
    ) -> Result<Self, FinError> {
        if rsi_period == 0 {
            return Err(FinError::InvalidPeriod(rsi_period));
        }
        if stoch_period == 0 {
            return Err(FinError::InvalidPeriod(stoch_period));
        }
        let rsi = Rsi::new("_srsi_inner", rsi_period)?;
        Ok(Self {
            name: name.into(),
            rsi_period,
            stoch_period,
            rsi,
            rsi_window: VecDeque::with_capacity(stoch_period),
        })
    }

    /// The RSI period used internally.
    pub fn rsi_period(&self) -> usize {
        self.rsi_period
    }

    /// The stochastic window period applied to RSI values.
    pub fn stoch_period(&self) -> usize {
        self.stoch_period
    }
}

impl Signal for StochRsi {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let rsi_val = match self.rsi.update(bar)? {
            SignalValue::Scalar(v) => v,
            SignalValue::Unavailable => return Ok(SignalValue::Unavailable),
        };

        self.rsi_window.push_back(rsi_val);
        if self.rsi_window.len() > self.stoch_period {
            self.rsi_window.pop_front();
        }
        if self.rsi_window.len() < self.stoch_period {
            return Ok(SignalValue::Unavailable);
        }

        let max_rsi = self
            .rsi_window
            .iter()
            .copied()
            .fold(Decimal::MIN, Decimal::max);
        let min_rsi = self
            .rsi_window
            .iter()
            .copied()
            .fold(Decimal::MAX, Decimal::min);

        let range = max_rsi - min_rsi;
        if range == Decimal::ZERO {
            return Ok(SignalValue::Scalar(Decimal::from(50u32)));
        }

        let stoch = (rsi_val - min_rsi)
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::ONE_HUNDRED)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(stoch))
    }

    fn is_ready(&self) -> bool {
        self.rsi_window.len() >= self.stoch_period && self.rsi.is_ready()
    }

    fn period(&self) -> usize {
        self.rsi_period
    }

    fn reset(&mut self) {
        self.rsi.reset();
        self.rsi_window.clear();
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
    fn test_stochrsi_period_0_fails() {
        assert!(StochRsi::new("s", 0, 3).is_err());
        assert!(StochRsi::new("s", 14, 0).is_err());
    }

    #[test]
    fn test_stochrsi_unavailable_before_warmup() {
        let mut sr = StochRsi::new("sr", 3, 3).unwrap();
        // RSI needs 3+1=4 bars, then stoch needs 3 RSI values → 6 bars minimum
        for i in 0..5u32 {
            assert_eq!(
                sr.update_bar(&bar(&(100 + i).to_string())).unwrap(),
                SignalValue::Unavailable
            );
        }
    }

    #[test]
    fn test_stochrsi_produces_scalar_after_warmup() {
        let mut sr = StochRsi::new("sr", 3, 3).unwrap();
        let mut got_scalar = false;
        for i in 0..20u32 {
            if let SignalValue::Scalar(_) = sr.update_bar(&bar(&(100 + i).to_string())).unwrap() {
                got_scalar = true;
                break;
            }
        }
        assert!(got_scalar);
    }

    #[test]
    fn test_stochrsi_in_range_0_to_100() {
        let mut sr = StochRsi::new("sr", 3, 3).unwrap();
        let prices = [
            "100", "102", "101", "103", "102", "104", "103", "101", "99", "100", "102", "104",
        ];
        for c in &prices {
            if let SignalValue::Scalar(v) = sr.update_bar(&bar(c)).unwrap() {
                assert!(v >= dec!(0), "StochRSI < 0: {v}");
                assert!(v <= dec!(100), "StochRSI > 100: {v}");
            }
        }
    }

    #[test]
    fn test_stochrsi_reset_clears_state() {
        let mut sr = StochRsi::new("sr", 3, 3).unwrap();
        for i in 0..20u32 {
            sr.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(sr.is_ready());
        sr.reset();
        assert!(!sr.is_ready());
        assert_eq!(
            sr.update_bar(&bar("100")).unwrap(),
            SignalValue::Unavailable
        );
    }

    #[test]
    fn test_stochrsi_period_accessors() {
        let sr = StochRsi::new("sr14_3", 14, 3).unwrap();
        assert_eq!(sr.period(), 14);
        assert_eq!(sr.rsi_period(), 14);
        assert_eq!(sr.stoch_period(), 3);
    }
}
