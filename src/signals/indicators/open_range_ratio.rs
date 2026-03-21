//! Open Range Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Open Range Ratio.
///
/// Rolling average of the distance between the open and close as a fraction
/// of the total range. Measures how much of the bar's range is "used up"
/// by the open-to-close move (the body width).
///
/// Per-bar formula:
/// - `body = |close - open|`
/// - `range = high - low`
/// - `ratio = body / range` (0 when range == 0)
///
/// Rolling: `mean(ratio, period)` ∈ [0, 1]
///
/// - Near 1: bars have large bodies relative to their range (strong directional moves).
/// - Near 0: bars have small bodies (indecision, spinning tops, doji).
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenRangeRatio;
/// use fin_primitives::signals::Signal;
/// let orr = OpenRangeRatio::new("orr_14", 14).unwrap();
/// assert_eq!(orr.period(), 14);
/// ```
pub struct OpenRangeRatio {
    name: String,
    period: usize,
    ratios: VecDeque<Decimal>,
}

impl OpenRangeRatio {
    /// Constructs a new `OpenRangeRatio`.
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

impl Signal for OpenRangeRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let ratio = if range.is_zero() {
            Decimal::ZERO
        } else {
            let body = (bar.close - bar.open).abs();
            body.checked_div(range).ok_or(FinError::ArithmeticOverflow)?
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
        assert!(matches!(OpenRangeRatio::new("orr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut orr = OpenRangeRatio::new("orr", 3).unwrap();
        assert_eq!(orr.update_bar(&bar("10", "12", "8", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_full_body_gives_one() {
        // open=8, close=12, high=12, low=8 → body=4, range=4 → ratio=1
        let mut orr = OpenRangeRatio::new("orr", 3).unwrap();
        for _ in 0..3 {
            orr.update_bar(&bar("8", "12", "8", "12")).unwrap();
        }
        let v = orr.update_bar(&bar("8", "12", "8", "12")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_half_body() {
        // open=10, close=12, high=12, low=8 → body=2, range=4 → ratio=0.5
        let mut orr = OpenRangeRatio::new("orr", 3).unwrap();
        for _ in 0..3 {
            orr.update_bar(&bar("10", "12", "8", "12")).unwrap();
        }
        let v = orr.update_bar(&bar("10", "12", "8", "12")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_reset() {
        let mut orr = OpenRangeRatio::new("orr", 2).unwrap();
        orr.update_bar(&bar("10", "12", "8", "11")).unwrap();
        orr.update_bar(&bar("10", "12", "8", "11")).unwrap();
        assert!(orr.is_ready());
        orr.reset();
        assert!(!orr.is_ready());
    }
}
