//! Close Return Acceleration — rate of change of rolling return momentum.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Return Acceleration — second derivative of price: `momentum(N) - prev_momentum(N)`.
///
/// Computes the N-bar return (close-to-close momentum) and then measures how that
/// momentum is changing bar-over-bar:
/// - **Positive**: momentum increasing — acceleration in current trend direction.
/// - **Negative**: momentum decreasing — deceleration or reversal building.
/// - **Near zero**: constant momentum — steady trend pace.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseReturnAcceleration;
/// use fin_primitives::signals::Signal;
/// let cra = CloseReturnAcceleration::new("cra_10", 10).unwrap();
/// assert_eq!(cra.period(), 10);
/// ```
pub struct CloseReturnAcceleration {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    prev_momentum: Option<Decimal>,
}

impl CloseReturnAcceleration {
    /// Constructs a new `CloseReturnAcceleration`.
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
            prev_momentum: None,
        })
    }
}

impl Signal for CloseReturnAcceleration {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.prev_momentum.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() <= self.period {
            return Ok(SignalValue::Unavailable);
        }

        // Current N-bar momentum: (close_now - close_N_bars_ago) / close_N_bars_ago
        let close_now = *self.closes.back().unwrap();
        let close_n_ago = *self.closes.front().unwrap();

        if close_n_ago.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let momentum = (close_now - close_n_ago)
            .checked_div(close_n_ago)
            .ok_or(FinError::ArithmeticOverflow)?;

        let result = match self.prev_momentum {
            Some(pm) => SignalValue::Scalar(momentum - pm),
            None => SignalValue::Unavailable,
        };

        self.prev_momentum = Some(momentum);
        Ok(result)
    }

    fn reset(&mut self) {
        self.closes.clear();
        self.prev_momentum = None;
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
    fn test_cra_invalid_period() {
        assert!(CloseReturnAcceleration::new("cra", 0).is_err());
    }

    #[test]
    fn test_cra_unavailable_during_warmup() {
        let mut s = CloseReturnAcceleration::new("cra", 2).unwrap();
        assert_eq!(s.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(s.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
        assert!(!s.is_ready());
    }

    #[test]
    fn test_cra_constant_momentum_gives_zero() {
        // If momentum is constant (same % gain each period), acceleration = 0
        let mut s = CloseReturnAcceleration::new("cra", 1).unwrap();
        // Period=1: momentum = (close_now - close_prev) / close_prev
        // If we have 100, 102, 104.04: both return ~2% → acceleration ≈ 0
        s.update_bar(&bar("100")).unwrap(); // Unavailable (filling period)
        s.update_bar(&bar("102")).unwrap(); // prev_momentum = 0.02, returns Unavailable (first)
        // third bar: momentum = (104.04-102)/102 ≈ 0.02, acceleration ≈ 0
        if let SignalValue::Scalar(v) = s.update_bar(&bar("104.04")).unwrap() {
            assert!(v.abs() < dec!(0.001), "constant momentum should give ~0 accel: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cra_accelerating_returns_positive() {
        let mut s = CloseReturnAcceleration::new("cra", 1).unwrap();
        s.update_bar(&bar("100")).unwrap();
        s.update_bar(&bar("101")).unwrap(); // momentum = 1/100 = 0.01
        // Bigger return: 101 → 103 → momentum = 2/101 > 0.01 → acceleration > 0
        if let SignalValue::Scalar(v) = s.update_bar(&bar("103")).unwrap() {
            assert!(v > dec!(0), "larger return gives positive acceleration: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cra_reset() {
        let mut s = CloseReturnAcceleration::new("cra", 2).unwrap();
        for p in &["100","101","102","103","104"] { s.update_bar(&bar(p)).unwrap(); }
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
