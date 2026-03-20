//! Open-Close Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `open / close`.
///
/// Values > 1 indicate the bar consistently opened above where it closed (bearish bias).
/// Values < 1 indicate the bar consistently opened below where it closed (bullish bias).
pub struct OpenCloseRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenCloseRatio {
    /// Creates a new `OpenCloseRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenCloseRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let ratio = bar.open / bar.close;
        self.window.push_back(ratio);
        self.sum += ratio;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "OpenCloseRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: dec!(200),
            low: dec!(1),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_open_close_ratio_equal() {
        // open == close => ratio = 1 for all bars
        let mut sig = OpenCloseRatio::new(2).unwrap();
        sig.update(&bar("100", "100")).unwrap();
        let v = sig.update(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_open_close_ratio_bullish() {
        // open < close (bullish) => ratio < 1
        let mut sig = OpenCloseRatio::new(2).unwrap();
        sig.update(&bar("95", "100")).unwrap();
        let v = sig.update(&bar("95", "100")).unwrap();
        if let SignalValue::Scalar(x) = v {
            assert!(x < dec!(1), "bullish bars should produce ratio < 1, got {}", x);
        }
    }
}
