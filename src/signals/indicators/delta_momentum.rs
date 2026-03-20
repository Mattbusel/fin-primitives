//! Delta Momentum indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Delta Momentum — the rate of change of momentum (acceleration of price).
///
/// Momentum is defined as `close[t] - close[t - period]`. This indicator
/// computes the change in that momentum from one bar to the next:
///
/// ```text
/// momentum[t]       = close[t]   - close[t - period]
/// delta_momentum[t] = momentum[t] - momentum[t-1]
/// ```
///
/// A positive value indicates accelerating upward momentum; negative indicates
/// decelerating or reversing momentum.
///
/// Returns [`SignalValue::Unavailable`] until `period + 2` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::DeltaMomentum;
/// use fin_primitives::signals::Signal;
///
/// let dm = DeltaMomentum::new("dm", 10).unwrap();
/// assert_eq!(dm.period(), 10);
/// ```
pub struct DeltaMomentum {
    name: String,
    period: usize,
    closes: VecDeque<Decimal>,
    prev_momentum: Option<Decimal>,
}

impl DeltaMomentum {
    /// Constructs a new `DeltaMomentum`.
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

impl Signal for DeltaMomentum {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.prev_momentum.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period + 1 {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        // closes has exactly period+1 elements: [oldest, ..., current]
        let momentum = *self.closes.back().unwrap() - *self.closes.front().unwrap();

        let result = match self.prev_momentum {
            None => {
                self.prev_momentum = Some(momentum);
                SignalValue::Unavailable
            }
            Some(prev) => {
                let delta = momentum - prev;
                self.prev_momentum = Some(momentum);
                SignalValue::Scalar(delta)
            }
        };
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
    fn test_dm_invalid_period() {
        assert!(DeltaMomentum::new("dm", 0).is_err());
    }

    #[test]
    fn test_dm_unavailable_during_warm_up() {
        let mut dm = DeltaMomentum::new("dm", 3).unwrap();
        // Need period+2=5 bars for first Scalar
        for _ in 0..4 {
            assert_eq!(dm.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_dm_constant_price_gives_zero_delta() {
        let mut dm = DeltaMomentum::new("dm", 2).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 {
            last = dm.update_bar(&bar("100")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_dm_linear_trend_gives_zero_delta() {
        // Linear trend: close[t] = 100 + t → momentum = period (constant)
        // → delta_momentum = 0
        let mut dm = DeltaMomentum::new("dm", 2).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..6 {
            last = dm.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_dm_accelerating_trend_positive() {
        // Accelerating: each step grows larger
        let prices = ["100", "101", "103", "106", "110", "115"];
        let mut dm = DeltaMomentum::new("dm", 2).unwrap();
        let mut last = SignalValue::Unavailable;
        for p in &prices {
            last = dm.update_bar(&bar(p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert!(v > dec!(0), "accelerating trend should have positive delta: {}", v);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_dm_reset() {
        let mut dm = DeltaMomentum::new("dm", 3).unwrap();
        for i in 0u32..6 { dm.update_bar(&bar(&(100 + i).to_string())).unwrap(); }
        assert!(dm.is_ready());
        dm.reset();
        assert!(!dm.is_ready());
    }
}
