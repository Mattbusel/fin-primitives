//! Range-Return Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `|close - open| / (prev_close)` normalized by bar range.
///
/// Specifically: `(high - low) / prev_close * 100` — bar range as percentage of prior close.
/// Measures the normalised daily range independent of price level.
/// High values: large intraday swings relative to price (high activity).
/// Low values: tight range relative to price (compressed/low volatility).
/// Returns Unavailable until prev_close is set and window is full.
pub struct RangeReturnRatio {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl RangeReturnRatio {
    /// Creates a new `RangeReturnRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RangeReturnRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                let range_pct = (bar.high - bar.low) / pc * Decimal::ONE_HUNDRED;
                self.window.push_back(range_pct);
                self.sum += range_pct;
                if self.window.len() > self.period {
                    if let Some(old) = self.window.pop_front() {
                        self.sum -= old;
                    }
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum / Decimal::from(self.period as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) {
        self.prev_close = None;
        self.window.clear();
        self.sum = Decimal::ZERO;
    }
    fn name(&self) -> &str { "RangeReturnRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_rrr_basic() {
        // range=20, prev_close=100 → range_pct = 20%
        let mut sig = RangeReturnRatio::new(2).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap(); // prev_close set
        sig.update(&bar("110", "90", "100")).unwrap(); // range_pct = 20/100*100 = 20
        let v = sig.update(&bar("110", "90", "100")).unwrap(); // avg(20,20) = 20
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_rrr_zero_range() {
        // high=low=close → range=0 → pct=0
        let mut sig = RangeReturnRatio::new(2).unwrap();
        sig.update(&bar("100", "100", "100")).unwrap();
        sig.update(&bar("100", "100", "100")).unwrap();
        let v = sig.update(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
