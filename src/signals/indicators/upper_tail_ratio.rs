//! Upper Tail Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Upper Tail Ratio.
///
/// Rolling average of the upper wick as a fraction of the bar's total range.
/// The upper wick is `high - max(open, close)`; the range is `high - low`.
///
/// Per-bar formula:
/// - `upper_wick = high - max(open, close)`
/// - `range = high - low`
/// - `ratio = upper_wick / range` (0 when range == 0)
///
/// Rolling: `mean(ratio, period)` ∈ [0, 1]
///
/// - Near 1: price consistently rejects from highs (bearish pressure at top).
/// - Near 0: bars close near their high (bullish).
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpperTailRatio;
/// use fin_primitives::signals::Signal;
/// let utr = UpperTailRatio::new("utr_14", 14).unwrap();
/// assert_eq!(utr.period(), 14);
/// ```
pub struct UpperTailRatio {
    name: String,
    period: usize,
    ratios: VecDeque<Decimal>,
}

impl UpperTailRatio {
    /// Constructs a new `UpperTailRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, ratios: VecDeque::with_capacity(period) })
    }
}

impl Signal for UpperTailRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let ratio = if range.is_zero() {
            Decimal::ZERO
        } else {
            let body_top = bar.open.max(bar.close);
            let upper_wick = bar.high - body_top;
            upper_wick.checked_div(range).ok_or(FinError::ArithmeticOverflow)?
        };

        self.ratios.push_back(ratio);
        if self.ratios.len() > self.period {
            self.ratios.pop_front();
        }
        if self.ratios.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.ratios.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.ratios.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.ratios.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(UpperTailRatio::new("utr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut utr = UpperTailRatio::new("utr", 3).unwrap();
        assert_eq!(utr.update_bar(&bar("10", "12", "9", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_no_upper_wick_gives_zero() {
        // open=10, high=12, low=8, close=12 → body_top=12, upper_wick=0
        let mut utr = UpperTailRatio::new("utr", 3).unwrap();
        for _ in 0..3 {
            utr.update_bar(&bar("10", "12", "8", "12")).unwrap();
        }
        let v = utr.update_bar(&bar("10", "12", "8", "12")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_full_upper_wick_gives_half() {
        // open=8, high=12, low=8, close=8 → body_top=8, upper_wick=4, range=4 → ratio=1
        // But with open=close=10, high=12, low=8 → body_top=10, upper_wick=2, range=4 → 0.5
        let mut utr = UpperTailRatio::new("utr", 3).unwrap();
        for _ in 0..3 {
            utr.update_bar(&bar("10", "12", "8", "10")).unwrap();
        }
        let v = utr.update_bar(&bar("10", "12", "8", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_reset() {
        let mut utr = UpperTailRatio::new("utr", 2).unwrap();
        utr.update_bar(&bar("10", "12", "8", "11")).unwrap();
        utr.update_bar(&bar("10", "12", "8", "11")).unwrap();
        assert!(utr.is_ready());
        utr.reset();
        assert!(!utr.is_ready());
    }
}
