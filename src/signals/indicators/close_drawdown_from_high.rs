//! Close Drawdown From High indicator.
//!
//! Tracks the rolling maximum close over `period` bars and computes the
//! current close's drawdown from that peak, expressed as a percentage.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Percentage drawdown of current close from its rolling `period`-bar maximum.
///
/// ```text
/// peak  = max(close, N)
/// drawdown_pct = (close - peak) / peak × 100
/// ```
///
/// The value is always `≤ 0`. A value of `0` means the current close is at the
/// rolling high. A value of `-10` means the close is 10% below its period high.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have accumulated
/// or when the peak close is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseDrawdownFromHigh;
/// use fin_primitives::signals::Signal;
///
/// let cdfh = CloseDrawdownFromHigh::new("cdfh", 20).unwrap();
/// assert_eq!(cdfh.period(), 20);
/// assert!(!cdfh.is_ready());
/// ```
pub struct CloseDrawdownFromHigh {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl CloseDrawdownFromHigh {
    /// Constructs a new `CloseDrawdownFromHigh`.
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
            window: VecDeque::with_capacity(period),
        })
    }
}

impl crate::signals::Signal for CloseDrawdownFromHigh {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        if self.window.len() > self.period {
            self.window.pop_front();
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let peak = self.window.iter().copied().fold(Decimal::MIN, Decimal::max);

        if peak.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let drawdown = (bar.close - peak)
            .checked_div(peak)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(drawdown))
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

    fn bar(close: &str) -> OhlcvBar {
        let c = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: c, high: c, low: c, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cdfh_invalid_period() {
        assert!(CloseDrawdownFromHigh::new("cdfh", 0).is_err());
    }

    #[test]
    fn test_cdfh_unavailable_during_warmup() {
        let mut cdfh = CloseDrawdownFromHigh::new("cdfh", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(cdfh.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_cdfh_at_peak_returns_zero() {
        let mut cdfh = CloseDrawdownFromHigh::new("cdfh", 3).unwrap();
        cdfh.update_bar(&bar("100")).unwrap();
        cdfh.update_bar(&bar("95")).unwrap();
        // Current close is 110, the highest → drawdown = 0
        let v = cdfh.update_bar(&bar("110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cdfh_below_peak_negative() {
        let mut cdfh = CloseDrawdownFromHigh::new("cdfh", 3).unwrap();
        cdfh.update_bar(&bar("100")).unwrap();
        cdfh.update_bar(&bar("110")).unwrap(); // peak = 110
        // close=99 → (99-110)/110*100 = -10
        let v = cdfh.update_bar(&bar("99")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s < dec!(0), "below peak → negative drawdown: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cdfh_reset() {
        let mut cdfh = CloseDrawdownFromHigh::new("cdfh", 3).unwrap();
        for _ in 0..3 {
            cdfh.update_bar(&bar("100")).unwrap();
        }
        assert!(cdfh.is_ready());
        cdfh.reset();
        assert!(!cdfh.is_ready());
    }
}
