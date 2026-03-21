//! Volatility Bias indicator.
//!
//! Measures what fraction of the rolling true range sum comes from upward moves
//! versus downward moves, identifying directional volatility asymmetry.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling fraction of true range attributable to upward moves.
///
/// For each bar, the "up TR" is `max(0, high - prev_close)` and the "down TR"
/// is `max(0, prev_close - low)`. The bias is:
/// ```text
/// bias = sum(up_tr, N) / sum(total_tr, N)
/// ```
///
/// - Values near `1.0`: volatility is predominantly upward (bullish momentum).
/// - Values near `0.0`: volatility is predominantly downward (bearish momentum).
/// - Values near `0.5`: balanced volatility.
///
/// On the first bar (no prev_close), `up_tr = high - low` and `total_tr = high - low`
/// → bias seeds at `1.0` if the range is non-zero.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have accumulated or
/// when cumulative total TR is zero.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityBias;
/// use fin_primitives::signals::Signal;
///
/// let vb = VolatilityBias::new("vb", 14).unwrap();
/// assert_eq!(vb.period(), 14);
/// assert!(!vb.is_ready());
/// ```
pub struct VolatilityBias {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    up_window: VecDeque<Decimal>,
    total_window: VecDeque<Decimal>,
    up_sum: Decimal,
    total_sum: Decimal,
}

impl VolatilityBias {
    /// Constructs a new `VolatilityBias`.
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
            up_window: VecDeque::with_capacity(period),
            total_window: VecDeque::with_capacity(period),
            up_sum: Decimal::ZERO,
            total_sum: Decimal::ZERO,
        })
    }
}

impl crate::signals::Signal for VolatilityBias {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.up_window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let (up_tr, total_tr) = if let Some(pc) = self.prev_close {
            let up = (bar.high - pc).max(Decimal::ZERO);
            let dn = (pc - bar.low).max(Decimal::ZERO);
            let total = up.max(dn).max(bar.high - bar.low);
            (up, total)
        } else {
            let hl = bar.range();
            (hl, hl)
        };

        self.up_sum += up_tr;
        self.total_sum += total_tr;

        self.up_window.push_back(up_tr);
        self.total_window.push_back(total_tr);

        if self.up_window.len() > self.period {
            if let Some(old_u) = self.up_window.pop_front() {
                self.up_sum -= old_u;
            }
            if let Some(old_t) = self.total_window.pop_front() {
                self.total_sum -= old_t;
            }
        }

        self.prev_close = Some(bar.close);

        if self.up_window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if self.total_sum.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let bias = self.up_sum
            .checked_div(self.total_sum)
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(bias))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.up_window.clear();
        self.total_window.clear();
        self.up_sum = Decimal::ZERO;
        self.total_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(high: &str, low: &str, close: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(low.parse().unwrap()).unwrap(),
            high: Price::new(high.parse().unwrap()).unwrap(),
            low: Price::new(low.parse().unwrap()).unwrap(),
            close: Price::new(close.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vb_invalid_period() {
        assert!(VolatilityBias::new("vb", 0).is_err());
    }

    #[test]
    fn test_vb_unavailable_during_warmup() {
        let mut vb = VolatilityBias::new("vb", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(vb.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_vb_result_in_range() {
        let mut vb = VolatilityBias::new("vb", 3).unwrap();
        for _ in 0..4 {
            vb.update_bar(&bar("110", "90", "100")).unwrap();
        }
        let v = vb.update_bar(&bar("110", "90", "100")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s >= dec!(0) && s <= dec!(1), "bias out of [0,1]: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_vb_reset() {
        let mut vb = VolatilityBias::new("vb", 3).unwrap();
        for _ in 0..4 {
            vb.update_bar(&bar("110", "90", "100")).unwrap();
        }
        assert!(vb.is_ready());
        vb.reset();
        assert!(!vb.is_ready());
    }
}
