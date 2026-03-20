//! Smoothed Stochastic RSI indicator (%K and %D lines).
//!
//! Extends [`StochRsi`] with a 3-period SMA smoothing of the raw %K line to
//! produce the %D signal line. This is the form described in Chande & Kroll's
//! original paper and is commonly used for signal crossovers.

use crate::error::FinError;
use crate::signals::indicators::StochRsi;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Smoothed Stochastic RSI — raw %K line and a 3-period SMA (%D) of it.
///
/// `%K` is the raw [`StochRsi`] value; `%D = SMA(%K, smooth_period)`.
///
/// Returns [`SignalValue::Unavailable`] until both %K and enough %K values
/// are available for the %D average.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::StochRsiSmoothed;
/// use fin_primitives::signals::Signal;
///
/// let s = StochRsiSmoothed::new("srsi_smooth", 14, 3, 3).unwrap();
/// assert_eq!(s.period(), 14);
/// ```
pub struct StochRsiSmoothed {
    name: String,
    stoch_rsi: StochRsi,
    smooth_period: usize,
    k_window: VecDeque<Decimal>,
}

impl StochRsiSmoothed {
    /// Constructs a new `StochRsiSmoothed`.
    ///
    /// - `rsi_period`: lookback for the inner RSI (e.g. 14)
    /// - `stoch_period`: rolling window applied to RSI values (e.g. 3)
    /// - `smooth_period`: SMA period for the %D line (typically 3)
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if any period is 0.
    pub fn new(
        name: impl Into<String>,
        rsi_period: usize,
        stoch_period: usize,
        smooth_period: usize,
    ) -> Result<Self, FinError> {
        if smooth_period == 0 {
            return Err(FinError::InvalidPeriod(smooth_period));
        }
        let stoch_rsi = StochRsi::new("_srsi_inner", rsi_period, stoch_period)?;
        Ok(Self {
            name: name.into(),
            stoch_rsi,
            smooth_period,
            k_window: VecDeque::with_capacity(smooth_period),
        })
    }

    /// Returns `smooth_period` (the %D SMA length).
    pub fn smooth_period(&self) -> usize {
        self.smooth_period
    }
}

impl Signal for StochRsiSmoothed {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let k = match self.stoch_rsi.update(bar)? {
            SignalValue::Scalar(v) => v,
            SignalValue::Unavailable => return Ok(SignalValue::Unavailable),
        };

        self.k_window.push_back(k);
        if self.k_window.len() > self.smooth_period {
            self.k_window.pop_front();
        }
        if self.k_window.len() < self.smooth_period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.k_window.iter().copied().sum();
        let d = sum
            .checked_div(Decimal::from(self.smooth_period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(d))
    }

    fn is_ready(&self) -> bool {
        self.k_window.len() >= self.smooth_period && self.stoch_rsi.is_ready()
    }

    fn period(&self) -> usize {
        self.stoch_rsi.period()
    }

    fn reset(&mut self) {
        self.stoch_rsi.reset();
        self.k_window.clear();
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
    fn test_stochrsi_smoothed_period_0_fails() {
        assert!(StochRsiSmoothed::new("s", 14, 3, 0).is_err());
        assert!(StochRsiSmoothed::new("s", 0, 3, 3).is_err());
    }

    #[test]
    fn test_stochrsi_smoothed_unavailable_before_warmup() {
        // rsi_period=3 (+1 for RSI gain) + stoch_period-1=2 + smooth_period-1=2 = 7 bars before ready
        let mut s = StochRsiSmoothed::new("s", 3, 3, 3).unwrap();
        for i in 0..7u32 {
            assert_eq!(
                s.update_bar(&bar(&(100 + i).to_string())).unwrap(),
                SignalValue::Unavailable
            );
        }
        assert!(!s.is_ready());
    }

    #[test]
    fn test_stochrsi_smoothed_produces_value() {
        let mut s = StochRsiSmoothed::new("s", 3, 3, 3).unwrap();
        let mut got = false;
        for i in 0..30u32 {
            if let SignalValue::Scalar(_) = s.update_bar(&bar(&(100 + i).to_string())).unwrap() {
                got = true;
                break;
            }
        }
        assert!(got);
    }

    #[test]
    fn test_stochrsi_smoothed_in_range() {
        let mut s = StochRsiSmoothed::new("s", 3, 3, 3).unwrap();
        let prices = [
            "100", "102", "101", "103", "102", "104", "103", "101", "99", "100",
            "102", "104", "103", "105", "104",
        ];
        for c in &prices {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(c)).unwrap() {
                assert!(v >= dec!(0), "value < 0: {v}");
                assert!(v <= dec!(100), "value > 100: {v}");
            }
        }
    }

    #[test]
    fn test_stochrsi_smoothed_reset() {
        let mut s = StochRsiSmoothed::new("s", 3, 3, 3).unwrap();
        for i in 0..30u32 {
            s.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
