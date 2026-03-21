//! Price Compression Streak indicator.
//!
//! Counts the number of consecutive bars where the bar's range was narrower
//! than the prior bar's range, detecting sustained range contraction.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Price Compression Streak: consecutive bars with narrowing range.
///
/// Returns the count of the current unbroken run where each bar's
/// `(high - low)` is strictly less than the previous bar's `(high - low)`.
/// Resets to zero when a bar's range is >= the prior bar's range.
///
/// High values indicate sustained range compression — a classic precursor
/// to volatility expansion and breakout moves.
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior range).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] is never triggered.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceCompressionStreak;
/// use fin_primitives::signals::Signal;
///
/// let pcs = PriceCompressionStreak::new("pcs").unwrap();
/// assert_eq!(pcs.period(), 1);
/// assert!(!pcs.is_ready());
/// ```
pub struct PriceCompressionStreak {
    name: String,
    prev_range: Option<Decimal>,
    streak: u32,
    seen_bars: usize,
}

impl PriceCompressionStreak {
    /// Constructs a new `PriceCompressionStreak`.
    ///
    /// # Errors
    /// Never fails; returns `Ok` always.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_range: None, streak: 0, seen_bars: 0 })
    }
}

impl Signal for PriceCompressionStreak {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        self.seen_bars >= 2
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        self.seen_bars += 1;

        let Some(prev) = self.prev_range else {
            self.prev_range = Some(range);
            return Ok(SignalValue::Unavailable);
        };

        if range < prev {
            self.streak += 1;
        } else {
            self.streak = 0;
        }

        self.prev_range = Some(range);
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.prev_range = None;
        self.streak = 0;
        self.seen_bars = 0;
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
    fn test_pcs_first_bar_unavailable() {
        let mut pcs = PriceCompressionStreak::new("pcs").unwrap();
        assert_eq!(pcs.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!pcs.is_ready());
    }

    #[test]
    fn test_pcs_ready_after_second_bar() {
        let mut pcs = PriceCompressionStreak::new("pcs").unwrap();
        pcs.update_bar(&bar("110", "90")).unwrap();
        pcs.update_bar(&bar("108", "93")).unwrap();
        assert!(pcs.is_ready());
    }

    #[test]
    fn test_pcs_counts_narrowing_ranges() {
        let mut pcs = PriceCompressionStreak::new("pcs").unwrap();
        pcs.update_bar(&bar("120", "80")).unwrap(); // range 40
        assert_eq!(pcs.update_bar(&bar("115", "85")).unwrap(), SignalValue::Scalar(dec!(1))); // 30 < 40
        assert_eq!(pcs.update_bar(&bar("112", "88")).unwrap(), SignalValue::Scalar(dec!(2))); // 24 < 30
        assert_eq!(pcs.update_bar(&bar("111", "90")).unwrap(), SignalValue::Scalar(dec!(3))); // 21 < 24
    }

    #[test]
    fn test_pcs_resets_on_wider_range() {
        let mut pcs = PriceCompressionStreak::new("pcs").unwrap();
        pcs.update_bar(&bar("120", "80")).unwrap();
        pcs.update_bar(&bar("115", "85")).unwrap();
        pcs.update_bar(&bar("112", "88")).unwrap();
        // Wide bar: range = 40, breaks streak
        let v = pcs.update_bar(&bar("120", "80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pcs_equal_range_resets() {
        let mut pcs = PriceCompressionStreak::new("pcs").unwrap();
        pcs.update_bar(&bar("120", "100")).unwrap(); // range 20
        // Same range: not strictly narrower → streak = 0
        let v = pcs.update_bar(&bar("120", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pcs_period_is_one() {
        let pcs = PriceCompressionStreak::new("pcs").unwrap();
        assert_eq!(pcs.period(), 1);
    }

    #[test]
    fn test_pcs_reset() {
        let mut pcs = PriceCompressionStreak::new("pcs").unwrap();
        pcs.update_bar(&bar("110", "90")).unwrap();
        pcs.update_bar(&bar("108", "93")).unwrap();
        assert!(pcs.is_ready());
        pcs.reset();
        assert!(!pcs.is_ready());
    }
}
