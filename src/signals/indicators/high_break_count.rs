//! High Break Count indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High Break Count — the number of bars in the last `period` bars (excluding
/// the current bar) where the current bar's close exceeds that bar's high.
///
/// A higher count indicates the current close is punching through multiple
/// prior resistance levels, suggesting a strong breakout.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighBreakCount;
/// use fin_primitives::signals::Signal;
///
/// let hbc = HighBreakCount::new("hbc", 10).unwrap();
/// assert_eq!(hbc.period(), 10);
/// ```
pub struct HighBreakCount {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
}

impl HighBreakCount {
    /// Constructs a new `HighBreakCount`.
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
            highs: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for HighBreakCount {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.highs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Count how many stored highs the current close exceeds BEFORE updating
        let result = if self.highs.len() >= self.period {
            let count = self.highs.iter().filter(|&&h| bar.close > h).count();
            SignalValue::Scalar(Decimal::from(count as u32))
        } else {
            SignalValue::Unavailable
        };

        self.highs.push_back(bar.high);
        if self.highs.len() > self.period { self.highs.pop_front(); }

        Ok(result)
    }

    fn reset(&mut self) {
        self.highs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new("90".parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_hbc_invalid_period() {
        assert!(HighBreakCount::new("hbc", 0).is_err());
    }

    #[test]
    fn test_hbc_unavailable_before_warm_up() {
        let mut hbc = HighBreakCount::new("hbc", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(hbc.update_bar(&bar("110", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_hbc_no_breaks() {
        let mut hbc = HighBreakCount::new("hbc", 3).unwrap();
        // Fill with high=110 bars
        for _ in 0..3 { hbc.update_bar(&bar("110", "100")).unwrap(); }
        // close=100 < all highs (110) → count=0
        let result = hbc.update_bar(&bar("110", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hbc_all_breaks() {
        let mut hbc = HighBreakCount::new("hbc", 3).unwrap();
        // Fill with high=100 bars
        for _ in 0..3 { hbc.update_bar(&bar("100", "95")).unwrap(); }
        // close=150 > all highs (100) → count=3
        let result = hbc.update_bar(&bar("155", "150")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_hbc_reset() {
        let mut hbc = HighBreakCount::new("hbc", 3).unwrap();
        for _ in 0..3 { hbc.update_bar(&bar("110", "100")).unwrap(); }
        assert!(hbc.is_ready());
        hbc.reset();
        assert!(!hbc.is_ready());
    }
}
