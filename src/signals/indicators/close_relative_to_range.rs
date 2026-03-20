//! Close Relative to Range indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(close - rolling_low) / (rolling_high - rolling_low)`.
///
/// Measures where the current close sits within the N-bar price channel:
/// - 1.0: close at the rolling high
/// - 0.0: close at the rolling low
/// - 0.5: close at the midpoint of the channel
pub struct CloseRelativeToRange {
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl CloseRelativeToRange {
    /// Creates a new `CloseRelativeToRange` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for CloseRelativeToRange {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let rolling_high = self.highs.iter().cloned().fold(Decimal::MIN, Decimal::max);
        let rolling_low = self.lows.iter().cloned().fold(Decimal::MAX, Decimal::min);
        let channel = rolling_high - rolling_low;
        if channel.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::from_str_exact("0.5").unwrap()));
        }
        Ok(SignalValue::Scalar((bar.close - rolling_low) / channel))
    }

    fn is_ready(&self) -> bool { self.highs.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.highs.clear(); self.lows.clear(); }
    fn name(&self) -> &str { "CloseRelativeToRange" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_crtr_at_top() {
        // close = rolling_high → 1.0
        let mut sig = CloseRelativeToRange::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("120", "95", "120")).unwrap();
        // rolling_high=120, rolling_low=90, close=120 → (120-90)/(120-90) = 1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_crtr_at_bottom() {
        // close = rolling_low → 0.0
        let mut sig = CloseRelativeToRange::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap();
        let v = sig.update(&bar("105", "85", "85")).unwrap();
        // rolling_high=110, rolling_low=85, close=85 → (85-85)/(110-85) = 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
