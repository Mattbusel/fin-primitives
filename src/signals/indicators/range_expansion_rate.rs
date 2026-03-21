//! Range Expansion Rate indicator.
//!
//! Tracks the rolling SMA of (current bar range / previous bar range), measuring
//! whether bar ranges are systematically expanding or contracting over time.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Rolling SMA of `current_range / prev_range`.
///
/// For each bar (starting from the second) the raw ratio is:
/// ```text
/// raw = (high[t] - low[t]) / (high[t-1] - low[t-1])   when prev_range > 0
///     = 1.0                                             when prev_range == 0 (flat prev bar)
/// ```
///
/// Values above `1.0` indicate bar ranges are expanding on average (increasing
/// volatility). Values below `1.0` indicate contracting ranges (decreasing
/// volatility / compression). Values near `1.0` indicate stable range behavior.
///
/// Returns [`SignalValue::Unavailable`] until `period` ratios have accumulated
/// (requires `period + 1` bars total).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeExpansionRate;
/// use fin_primitives::signals::Signal;
///
/// let rer = RangeExpansionRate::new("rer", 10).unwrap();
/// assert_eq!(rer.period(), 10);
/// assert!(!rer.is_ready());
/// ```
pub struct RangeExpansionRate {
    name: String,
    period: usize,
    prev_range: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl RangeExpansionRate {
    /// Constructs a new `RangeExpansionRate`.
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
            prev_range: None,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for RangeExpansionRate {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let cur_range = bar.range();

        let result = if let Some(prev) = self.prev_range {
            let raw = if prev.is_zero() {
                Decimal::ONE
            } else {
                cur_range
                    .checked_div(prev)
                    .ok_or(FinError::ArithmeticOverflow)?
            };

            self.sum += raw;
            self.window.push_back(raw);

            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }

            if self.window.len() < self.period {
                SignalValue::Unavailable
            } else {
                #[allow(clippy::cast_possible_truncation)]
                let mean = self.sum
                    .checked_div(Decimal::from(self.period as u32))
                    .ok_or(FinError::ArithmeticOverflow)?;
                SignalValue::Scalar(mean)
            }
        } else {
            SignalValue::Unavailable
        };

        self.prev_range = Some(cur_range);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_range = None;
        self.window.clear();
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

    fn bar(high: &str, low: &str) -> OhlcvBar {
        let h = Price::new(high.parse().unwrap()).unwrap();
        let l = Price::new(low.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: l, high: h, low: l, close: h,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rer_invalid_period() {
        assert!(RangeExpansionRate::new("rer", 0).is_err());
    }

    #[test]
    fn test_rer_unavailable_during_warmup() {
        let mut rer = RangeExpansionRate::new("rer", 3).unwrap();
        // First bar: no prev → Unavailable
        assert_eq!(rer.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        // Bars 2 and 3: have ratio but window not full
        assert_eq!(rer.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(rer.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rer_constant_range_returns_one() {
        // All bars same range → ratio = 1.0 always → SMA = 1.0
        let mut rer = RangeExpansionRate::new("rer", 3).unwrap();
        for _ in 0..4 {
            rer.update_bar(&bar("110", "90")).unwrap();
        }
        let v = rer.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_rer_expanding_ranges_above_one() {
        // Ranges: 10, 20, 40, 80 → ratios: 2, 2, 2 → SMA = 2
        let mut rer = RangeExpansionRate::new("rer", 3).unwrap();
        rer.update_bar(&bar("110", "100")).unwrap(); // range=10
        rer.update_bar(&bar("120", "100")).unwrap(); // range=20, ratio=2
        rer.update_bar(&bar("140", "100")).unwrap(); // range=40, ratio=2
        let v = rer.update_bar(&bar("180", "100")).unwrap(); // range=80, ratio=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_rer_reset() {
        let mut rer = RangeExpansionRate::new("rer", 3).unwrap();
        for _ in 0..5 {
            rer.update_bar(&bar("110", "90")).unwrap();
        }
        assert!(rer.is_ready());
        rer.reset();
        assert!(!rer.is_ready());
    }
}
