//! Velocity Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Velocity Ratio.
///
/// Compares the most recent bar's absolute price change to the rolling average
/// absolute price change. Values > 1 indicate the current bar moved faster
/// than average; values < 1 indicate a slower-than-average move.
///
/// Per-bar formula: `move_i = |close_i - close_{i-1}|`
///
/// Rolling: `velocity_ratio = move_t / mean(move, period)`
///
/// Returns `SignalValue::Unavailable` until `period + 1` closes accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VelocityRatio;
/// use fin_primitives::signals::Signal;
/// let vr = VelocityRatio::new("vr_14", 14).unwrap();
/// assert_eq!(vr.period(), 14);
/// ```
pub struct VelocityRatio {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    moves: VecDeque<Decimal>,
}

impl VelocityRatio {
    /// Constructs a new `VelocityRatio`.
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
            closes: VecDeque::with_capacity(2),
            moves: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VelocityRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > 2 {
            self.closes.pop_front();
        }
        if self.closes.len() < 2 {
            return Ok(SignalValue::Unavailable);
        }

        let m = (self.closes[1] - self.closes[0]).abs();
        self.moves.push_back(m);
        if self.moves.len() > self.period {
            self.moves.pop_front();
        }
        if self.moves.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.moves.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let mean = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if mean.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ONE));
        }

        let ratio = m.checked_div(mean).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool {
        self.moves.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.moves.clear();
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
        assert!(matches!(VelocityRatio::new("vr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_ready() {
        let mut vr = VelocityRatio::new("vr", 3).unwrap();
        assert_eq!(vr.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(vr.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_uniform_moves_give_one() {
        // All moves = 1, so current move = mean → ratio = 1
        let mut vr = VelocityRatio::new("vr", 3).unwrap();
        vr.update_bar(&bar("100")).unwrap();
        vr.update_bar(&bar("101")).unwrap();
        vr.update_bar(&bar("102")).unwrap();
        let v = vr.update_bar(&bar("103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_larger_move_above_one() {
        let mut vr = VelocityRatio::new("vr", 3).unwrap();
        vr.update_bar(&bar("100")).unwrap();
        vr.update_bar(&bar("101")).unwrap();
        vr.update_bar(&bar("102")).unwrap();
        vr.update_bar(&bar("103")).unwrap();
        // big jump: current move = 10, mean of window [1,1,10] = 4, ratio = 2.5
        let v = vr.update_bar(&bar("113")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(1));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut vr = VelocityRatio::new("vr", 2).unwrap();
        for i in 100..103u32 {
            vr.update_bar(&bar(&i.to_string())).unwrap();
        }
        assert!(vr.is_ready());
        vr.reset();
        assert!(!vr.is_ready());
    }
}
