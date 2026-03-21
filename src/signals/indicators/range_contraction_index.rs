//! Range Contraction Index indicator.
//!
//! Measures current bar range relative to the minimum range seen in a rolling
//! window. Detects how far above the recent tightest range the market currently is.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Contraction Index — `current_range / min_range_in_window`.
///
/// ```text
/// RCI[t] = range[t] / min(range[t-period+1 .. t])
/// ```
///
/// - **= 1.0**: current bar has the tightest range in the window — maximum
///   compression, classic breakout setup.
/// - **> 1.0**: current range is larger than the recent minimum — expanding
///   away from the tightest point.
/// - **High value**: recent range has expanded significantly vs the tight zone.
///
/// Returns [`SignalValue::Unavailable`] when the minimum range is zero (all
/// flat bars), or until `period` bars are collected.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeContractionIndex;
/// use fin_primitives::signals::Signal;
/// let rci = RangeContractionIndex::new("rci_20", 20).unwrap();
/// assert_eq!(rci.period(), 20);
/// ```
pub struct RangeContractionIndex {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl RangeContractionIndex {
    /// Constructs a new `RangeContractionIndex`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for RangeContractionIndex {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        self.window.push_back(range);
        if self.window.len() > self.period {
            self.window.pop_front();
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let min_range = self.window.iter().copied().min().unwrap_or(Decimal::ZERO);
        if min_range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let rci = range
            .checked_div(min_range)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(rci))
    }

    fn reset(&mut self) {
        self.window.clear();
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
    fn test_rci_invalid_period() {
        assert!(RangeContractionIndex::new("rci", 0).is_err());
    }

    #[test]
    fn test_rci_unavailable_during_warmup() {
        let mut rci = RangeContractionIndex::new("rci", 3).unwrap();
        assert_eq!(rci.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(rci.update_bar(&bar("108", "92")).unwrap(), SignalValue::Unavailable);
        assert!(!rci.is_ready());
    }

    #[test]
    fn test_rci_current_is_minimum_gives_one() {
        // When the current bar has the smallest range, RCI = 1
        let mut rci = RangeContractionIndex::new("rci", 3).unwrap();
        rci.update_bar(&bar("120", "80")).unwrap(); // range 40
        rci.update_bar(&bar("115", "85")).unwrap(); // range 30
        if let SignalValue::Scalar(v) = rci.update_bar(&bar("110", "90")).unwrap() {
            // range 20 = min → 20/20 = 1
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rci_expansion_above_one() {
        // After tight period, a wide bar → RCI > 1
        let mut rci = RangeContractionIndex::new("rci", 3).unwrap();
        rci.update_bar(&bar("105", "95")).unwrap(); // range 10
        rci.update_bar(&bar("104", "96")).unwrap(); // range 8
        if let SignalValue::Scalar(v) = rci.update_bar(&bar("120", "80")).unwrap() {
            // range 40, min = 8 → 40/8 = 5
            assert_eq!(v, dec!(5));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rci_sliding_window() {
        let mut rci = RangeContractionIndex::new("rci", 2).unwrap();
        rci.update_bar(&bar("120", "80")).unwrap(); // range 40
        rci.update_bar(&bar("106", "94")).unwrap(); // range 12; window=[40,12], min=12, rci=12/12=1
        if let SignalValue::Scalar(v) = rci.update_bar(&bar("130", "70")).unwrap() {
            // range 60; window=[12,60], min=12; rci=60/12=5
            assert_eq!(v, dec!(5));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rci_reset() {
        let mut rci = RangeContractionIndex::new("rci", 2).unwrap();
        rci.update_bar(&bar("110", "90")).unwrap();
        rci.update_bar(&bar("108", "92")).unwrap();
        assert!(rci.is_ready());
        rci.reset();
        assert!(!rci.is_ready());
    }
}
