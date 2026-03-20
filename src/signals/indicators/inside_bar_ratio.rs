//! Inside Bar Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling fraction of bars where `high < prev_high AND low > prev_low` (inside bars).
///
/// Inside bars indicate consolidation and reduced volatility.
/// High inside bar ratios suggest market indecision or compression before a breakout.
pub struct InsideBarRatio {
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl InsideBarRatio {
    /// Creates a new `InsideBarRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_high: None,
            prev_low: None,
            window: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for InsideBarRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            let inside: u8 = if bar.high < ph && bar.low > pl { 1 } else { 0 };
            self.window.push_back(inside);
            self.count += inside as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let ratio = Decimal::from(self.count as u32) / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_high = None; self.prev_low = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "InsideBarRatio" }
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
    fn test_inside_bar_ratio_all_inside() {
        let mut sig = InsideBarRatio::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap(); // seeds prev
        sig.update(&bar("108", "92")).unwrap(); // inside ✓
        let v = sig.update(&bar("106", "94")).unwrap(); // inside ✓ → 2/2 = 1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_inside_bar_ratio_none_inside() {
        let mut sig = InsideBarRatio::new(2).unwrap();
        sig.update(&bar("100", "95")).unwrap(); // seeds prev
        sig.update(&bar("115", "85")).unwrap(); // outside ✗
        let v = sig.update(&bar("120", "80")).unwrap(); // outside ✗ → 0/2 = 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
