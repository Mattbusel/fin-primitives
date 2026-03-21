//! Lower Tail Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Lower Tail Ratio.
///
/// Rolling average of the lower wick as a fraction of the bar's total range.
/// The lower wick is `min(open, close) - low`; the range is `high - low`.
///
/// Per-bar formula:
/// - `lower_wick = min(open, close) - low`
/// - `range = high - low`
/// - `ratio = lower_wick / range` (0 when range == 0)
///
/// Rolling: `mean(ratio, period)` ∈ [0, 1]
///
/// - Near 1: price consistently rejects from lows (bullish support at bottom).
/// - Near 0: bars close near their low (bearish).
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LowerTailRatio;
/// use fin_primitives::signals::Signal;
/// let ltr = LowerTailRatio::new("ltr_14", 14).unwrap();
/// assert_eq!(ltr.period(), 14);
/// ```
pub struct LowerTailRatio {
    name: String,
    period: usize,
    ratios: VecDeque<Decimal>,
}

impl LowerTailRatio {
    /// Constructs a new `LowerTailRatio`.
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

impl Signal for LowerTailRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let ratio = if range.is_zero() {
            Decimal::ZERO
        } else {
            let body_bottom = bar.open.min(bar.close);
            let lower_wick = body_bottom - bar.low;
            lower_wick.checked_div(range).ok_or(FinError::ArithmeticOverflow)?
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
        assert!(matches!(LowerTailRatio::new("ltr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut ltr = LowerTailRatio::new("ltr", 3).unwrap();
        assert_eq!(ltr.update_bar(&bar("10", "12", "9", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_no_lower_wick_gives_zero() {
        // open=8, high=12, low=8, close=10 → body_bottom=8, lower_wick=0
        let mut ltr = LowerTailRatio::new("ltr", 3).unwrap();
        for _ in 0..3 {
            ltr.update_bar(&bar("8", "12", "8", "10")).unwrap();
        }
        let v = ltr.update_bar(&bar("8", "12", "8", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_half_lower_wick() {
        // open=10, high=12, low=8, close=10 → body_bottom=10, lower_wick=2, range=4 → 0.5
        let mut ltr = LowerTailRatio::new("ltr", 3).unwrap();
        for _ in 0..3 {
            ltr.update_bar(&bar("10", "12", "8", "10")).unwrap();
        }
        let v = ltr.update_bar(&bar("10", "12", "8", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_reset() {
        let mut ltr = LowerTailRatio::new("ltr", 2).unwrap();
        ltr.update_bar(&bar("10", "12", "8", "11")).unwrap();
        ltr.update_bar(&bar("10", "12", "8", "11")).unwrap();
        assert!(ltr.is_ready());
        ltr.reset();
        assert!(!ltr.is_ready());
    }
}
