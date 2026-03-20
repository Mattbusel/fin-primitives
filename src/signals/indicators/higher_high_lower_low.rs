//! Higher High Lower Low indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Detects bar pattern: +1 if higher high AND lower low (outside bar),
/// -1 if lower high AND higher low (inside bar), 0 otherwise.
///
/// Outside bars (expanding range): indicate volatility breakout.
/// Inside bars (contracting range): indicate consolidation / indecision.
/// Returns Unavailable until the second bar.
pub struct HigherHighLowerLow {
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
}

impl HigherHighLowerLow {
    /// Creates a new `HigherHighLowerLow` indicator.
    pub fn new() -> Self {
        Self { prev_high: None, prev_low: None }
    }
}

impl Default for HigherHighLowerLow {
    fn default() -> Self { Self::new() }
}

impl Signal for HigherHighLowerLow {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            let outside = bar.high > ph && bar.low < pl;
            let inside = bar.high < ph && bar.low > pl;
            let val: i32 = if outside { 1 } else if inside { -1 } else { 0 };
            SignalValue::Scalar(Decimal::from(val))
        } else {
            SignalValue::Unavailable
        };
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        Ok(result)
    }

    fn is_ready(&self) -> bool { self.prev_high.is_some() }
    fn period(&self) -> usize { 2 }
    fn reset(&mut self) { self.prev_high = None; self.prev_low = None; }
    fn name(&self) -> &str { "HigherHighLowerLow" }
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
    fn test_hhll_outside_bar() {
        let mut sig = HigherHighLowerLow::new();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("115", "85")).unwrap(); // higher high AND lower low
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_hhll_inside_bar() {
        let mut sig = HigherHighLowerLow::new();
        sig.update(&bar("115", "85")).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap(); // lower high AND higher low
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_hhll_neutral() {
        let mut sig = HigherHighLowerLow::new();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("115", "91")).unwrap(); // higher high but NOT lower low
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
