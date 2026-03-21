//! Range-to-ATR Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range-to-ATR Ratio.
///
/// Compares the current bar's simple range (high − low) to the rolling Average True Range
/// over the same period. Unlike Volatility Adjusted Range (which uses mean range), this
/// indicator uses the true range to account for overnight gaps.
///
/// Formula:
/// - `true_range = max(high, prev_close) - min(low, prev_close)`
/// - `atr = mean(true_range, period)`
/// - `ratio = current_range / atr`
///
/// - > 1.0: current bar's range exceeds the average true range.
/// - < 1.0: current bar's range is below average.
/// - = 0.0: ATR is zero.
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeToAtrRatio;
/// use fin_primitives::signals::Signal;
/// let r = RangeToAtrRatio::new("rtar_14", 14).unwrap();
/// assert_eq!(r.period(), 14);
/// ```
pub struct RangeToAtrRatio {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    true_ranges: VecDeque<Decimal>,
}

impl RangeToAtrRatio {
    /// Constructs a new `RangeToAtrRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            true_ranges: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for RangeToAtrRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let current_range = bar.high - bar.low;

        let tr = if let Some(prev_c) = self.prev_close {
            let high_ext = bar.high.max(prev_c);
            let low_ext = bar.low.min(prev_c);
            high_ext - low_ext
        } else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };

        self.prev_close = Some(bar.close);
        self.true_ranges.push_back(tr);
        if self.true_ranges.len() > self.period {
            self.true_ranges.pop_front();
        }
        if self.true_ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.true_ranges.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let atr = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if atr.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let ratio = current_range.checked_div(atr).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool {
        self.true_ranges.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.true_ranges.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(RangeToAtrRatio::new("rtar", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut r = RangeToAtrRatio::new("rtar", 3).unwrap();
        assert_eq!(r.update_bar(&bar("12", "10", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_equal_ranges_gives_one() {
        let mut r = RangeToAtrRatio::new("rtar", 3).unwrap();
        // Feed period+1 bars with same range=2
        for _ in 0..4 {
            r.update_bar(&bar("12", "10", "11")).unwrap();
        }
        // Same range as ATR → ratio = 1
        let v = r.update_bar(&bar("12", "10", "11")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_reset() {
        let mut r = RangeToAtrRatio::new("rtar", 2).unwrap();
        r.update_bar(&bar("12", "10", "11")).unwrap();
        r.update_bar(&bar("12", "10", "11")).unwrap();
        r.update_bar(&bar("12", "10", "11")).unwrap();
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
