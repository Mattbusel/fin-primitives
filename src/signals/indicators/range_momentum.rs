//! Range Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// N-bar change in rolling average bar range.
///
/// `avg_range(t) - avg_range(t-N)` where avg_range is a K-period simple average.
/// Positive: ranges expanding over N bars (volatility accelerating).
/// Negative: ranges contracting over N bars (volatility decelerating).
pub struct RangeMomentum {
    avg_period: usize,
    mom_period: usize,
    bar_window: VecDeque<Decimal>, // raw ranges
    range_sum: Decimal,
    avg_history: VecDeque<Decimal>, // history of avg_range values
}

impl RangeMomentum {
    /// Creates a new `RangeMomentum`.
    ///
    /// `avg_period`: period for computing rolling average range.
    /// `mom_period`: look-back for momentum of that average.
    pub fn new(avg_period: usize, mom_period: usize) -> Result<Self, FinError> {
        if avg_period == 0 || mom_period == 0 {
            return Err(FinError::InvalidPeriod(if avg_period == 0 { avg_period } else { mom_period }));
        }
        Ok(Self {
            avg_period,
            mom_period,
            bar_window: VecDeque::with_capacity(avg_period),
            range_sum: Decimal::ZERO,
            avg_history: VecDeque::with_capacity(mom_period + 1),
        })
    }
}

impl Signal for RangeMomentum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.bar_window.push_back(range);
        self.range_sum += range;
        if self.bar_window.len() > self.avg_period {
            if let Some(old) = self.bar_window.pop_front() {
                self.range_sum -= old;
            }
        }
        if self.bar_window.len() < self.avg_period {
            return Ok(SignalValue::Unavailable);
        }

        let avg_range = self.range_sum / Decimal::from(self.avg_period as u32);
        self.avg_history.push_back(avg_range);
        if self.avg_history.len() > self.mom_period + 1 {
            self.avg_history.pop_front();
        }
        if self.avg_history.len() < self.mom_period + 1 {
            return Ok(SignalValue::Unavailable);
        }

        let current = *self.avg_history.back().unwrap();
        let base = *self.avg_history.front().unwrap();
        Ok(SignalValue::Scalar(current - base))
    }

    fn is_ready(&self) -> bool { self.avg_history.len() >= self.mom_period + 1 }
    fn period(&self) -> usize { self.avg_period + self.mom_period }
    fn reset(&mut self) {
        self.bar_window.clear();
        self.range_sum = Decimal::ZERO;
        self.avg_history.clear();
    }
    fn name(&self) -> &str { "RangeMomentum" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_rm_constant_range_zero() {
        // Constant range → avg_range constant → momentum = 0
        let mut sig = RangeMomentum::new(2, 2).unwrap();
        for _ in 0..5 {
            sig.update(&bar("110", "90")).unwrap();
        }
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rm_expanding_positive() {
        // Range grows: 10, 20, 30... → momentum positive
        let mut sig = RangeMomentum::new(1, 1).unwrap();
        sig.update(&bar("105", "95")).unwrap(); // range=10, avg=10, history=[10]
        sig.update(&bar("110", "90")).unwrap(); // range=20, avg=20, history=[10,20]
        let v = sig.update(&bar("115", "85")).unwrap(); // range=30, avg=30, history=[20,30]
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }
}
