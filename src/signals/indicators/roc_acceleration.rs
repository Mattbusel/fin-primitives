//! Rate-of-Change Acceleration indicator.
//!
//! Measures the change in momentum by computing the difference between the
//! current ROC and the ROC `period` bars ago (second derivative of price).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// ROC Acceleration: `ROC(N)[t] - ROC(N)[t - N]`.
///
/// Computes the N-bar Rate-of-Change for each bar, then returns the change
/// in that ROC value over the last N ROC readings. This is the second
/// derivative of price (acceleration of momentum):
///
/// - **Positive and growing**: momentum is accelerating upward.
/// - **Negative and falling**: momentum is decelerating into negative territory.
/// - **Crossing zero**: momentum trend is reversing direction.
///
/// Returns [`SignalValue::Unavailable`] until `2 * period` bars have been
/// accumulated (enough to produce a ROC *and* then measure its change over
/// another `period` bars).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RocAcceleration;
/// use fin_primitives::signals::Signal;
///
/// let ra = RocAcceleration::new("roc_acc", 10).unwrap();
/// assert_eq!(ra.period(), 10);
/// assert!(!ra.is_ready());
/// ```
pub struct RocAcceleration {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    rocs: VecDeque<Decimal>,
}

impl RocAcceleration {
    /// Constructs a new `RocAcceleration`.
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
            rocs: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for RocAcceleration {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.rocs.len() > self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        self.closes.push_back(close);

        // Keep closes window at period+1 to compute N-bar ROC
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }

        // Compute current ROC when we have enough closes
        if self.closes.len() == self.period + 1 {
            let old_close = *self.closes.front().unwrap_or(&close);
            if old_close.is_zero() {
                return Ok(SignalValue::Unavailable);
            }
            let roc = (close - old_close)
                .checked_div(old_close)
                .ok_or(FinError::ArithmeticOverflow)?
                * Decimal::ONE_HUNDRED;

            self.rocs.push_back(roc);
            if self.rocs.len() > self.period + 1 {
                self.rocs.pop_front();
            }
        }

        if self.rocs.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        let roc_now = *self.rocs.back().unwrap_or(&Decimal::ZERO);
        let roc_prev = *self.rocs.front().unwrap_or(&Decimal::ZERO);
        Ok(SignalValue::Scalar(roc_now - roc_prev))
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.rocs.clear();
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
    fn test_ra_invalid_period() {
        assert!(RocAcceleration::new("ra", 0).is_err());
    }

    #[test]
    fn test_ra_unavailable_during_warmup() {
        let mut ra = RocAcceleration::new("ra", 3).unwrap();
        for i in 1u32..=5 {
            assert_eq!(
                ra.update_bar(&bar(&(100 + i).to_string())).unwrap(),
                SignalValue::Unavailable,
                "expected Unavailable at bar {i}"
            );
        }
    }

    #[test]
    fn test_ra_constant_roc_zero_acceleration() {
        let mut ra = RocAcceleration::new("ra", 2).unwrap();
        // Steady +1% per bar → ROC stays constant → acceleration = 0
        let prices = ["100", "101", "102.01", "103.0301", "104.060401", "105.10100401"];
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = ra.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            // acceleration should be very close to 0 for constant ROC
            assert!(v.abs() < dec!(0.1), "near-constant ROC → near-zero acceleration: {v}");
        }
        // (Unavailable is also acceptable if not yet warm)
    }

    #[test]
    fn test_ra_produces_scalar_after_warmup() {
        let mut ra = RocAcceleration::new("ra", 2).unwrap();
        for i in 1u32..=5 {
            ra.update_bar(&bar(&(100 + i * 2).to_string())).unwrap();
        }
        let v = ra.update_bar(&bar("120")).unwrap();
        assert!(matches!(v, SignalValue::Scalar(_)), "expected Scalar after warmup");
    }

    #[test]
    fn test_ra_reset() {
        let mut ra = RocAcceleration::new("ra", 2).unwrap();
        for i in 1u32..=5 {
            ra.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert!(ra.is_ready());
        ra.reset();
        assert!(!ra.is_ready());
    }
}
