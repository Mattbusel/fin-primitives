//! Bearish Bar Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling fraction of bars where `close < open` (bearish/down bars).
///
/// Complement of bullish bar ratio. High values suggest persistent selling pressure.
pub struct BearishBarRatio {
    period: usize,
    window: VecDeque<u8>,
    count: usize,
}

impl BearishBarRatio {
    /// Creates a new `BearishBarRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for BearishBarRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let bearish: u8 = if bar.close < bar.open { 1 } else { 0 };
        self.window.push_back(bearish);
        self.count += bearish as usize;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.count -= old as usize;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let ratio = Decimal::from(self.count as u32) / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "BearishBarRatio" }
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
    fn test_bearish_bar_ratio_all_bearish() {
        let mut sig = BearishBarRatio::new(3).unwrap();
        sig.update(&bar("110", "100")).unwrap();
        sig.update(&bar("110", "100")).unwrap();
        let v = sig.update(&bar("110", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bearish_bar_ratio_none_bearish() {
        let mut sig = BearishBarRatio::new(3).unwrap();
        sig.update(&bar("100", "110")).unwrap();
        sig.update(&bar("100", "110")).unwrap();
        let v = sig.update(&bar("100", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bearish_bar_ratio_half() {
        let mut sig = BearishBarRatio::new(2).unwrap();
        sig.update(&bar("100", "110")).unwrap(); // bullish
        let v = sig.update(&bar("110", "100")).unwrap(); // bearish → 1/2
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }
}
