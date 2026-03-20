//! Average Bar Range — rolling SMA of the bar's high-low range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Average Bar Range — `SMA(high - low, period)`.
///
/// A simple rolling average of each bar's high-low range (not True Range).
/// Unlike ATR, this ignores overnight gaps, making it useful for intraday
/// instruments where gaps are minimal.
///
/// - **Rising**: bars are getting wider on average (volatility expansion).
/// - **Falling**: bars are narrowing (volatility contraction).
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AverageBarRange;
/// use fin_primitives::signals::Signal;
/// let abr = AverageBarRange::new("abr_14", 14).unwrap();
/// assert_eq!(abr.period(), 14);
/// ```
pub struct AverageBarRange {
    name: String,
    period: usize,
    ranges: VecDeque<Decimal>,
    sum: Decimal,
}

impl AverageBarRange {
    /// Constructs a new `AverageBarRange`.
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
            ranges: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for AverageBarRange {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.ranges.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.sum += range;
        self.ranges.push_back(range);
        if self.ranges.len() > self.period {
            let removed = self.ranges.pop_front().unwrap();
            self.sum -= removed;
        }
        if self.ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(avg))
    }

    fn reset(&mut self) {
        self.ranges.clear();
        self.sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_abr_invalid_period() {
        assert!(AverageBarRange::new("abr", 0).is_err());
    }

    #[test]
    fn test_abr_unavailable_before_period() {
        let mut abr = AverageBarRange::new("abr", 3).unwrap();
        assert_eq!(abr.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(abr.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!abr.is_ready());
    }

    #[test]
    fn test_abr_constant_range_known_value() {
        // Each bar has range=20 → average=20
        let mut abr = AverageBarRange::new("abr", 3).unwrap();
        for _ in 0..3 {
            abr.update_bar(&bar("110", "90")).unwrap();
        }
        let v = abr.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_abr_non_negative() {
        let mut abr = AverageBarRange::new("abr", 5).unwrap();
        let bars = [
            bar("110", "90"), bar("108", "92"), bar("115", "85"),
            bar("102", "98"), bar("112", "88"), bar("107", "93"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = abr.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "average range must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_abr_rolling_window() {
        // [20, 20, 10] → avg=50/3, then slides out first 20, adds 10: [20, 10, 10] → 40/3
        let mut abr = AverageBarRange::new("abr", 3).unwrap();
        abr.update_bar(&bar("110", "90")).unwrap(); // range=20
        abr.update_bar(&bar("110", "90")).unwrap(); // range=20
        abr.update_bar(&bar("105", "95")).unwrap(); // range=10 → avg=(20+20+10)/3
        let v1 = abr.update_bar(&bar("105", "95")).unwrap(); // range=10 → avg=(20+10+10)/3
        if let SignalValue::Scalar(r) = v1 {
            let expected = (dec!(20) + dec!(10) + dec!(10)) / dec!(3);
            assert!((r - expected).abs() < dec!(0.001), "expected {expected}, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_abr_reset() {
        let mut abr = AverageBarRange::new("abr", 3).unwrap();
        for _ in 0..4 {
            abr.update_bar(&bar("110", "90")).unwrap();
        }
        assert!(abr.is_ready());
        abr.reset();
        assert!(!abr.is_ready());
    }

    #[test]
    fn test_abr_period_and_name() {
        let abr = AverageBarRange::new("my_abr", 14).unwrap();
        assert_eq!(abr.period(), 14);
        assert_eq!(abr.name(), "my_abr");
    }
}
