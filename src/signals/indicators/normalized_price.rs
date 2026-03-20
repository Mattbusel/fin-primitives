//! Normalized Price indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Normalized Price — position of the current close within the n-bar high-low range (0 to 100).
///
/// ```text
/// NP = (close - lowest_low(period)) / (highest_high(period) - lowest_low(period)) * 100
/// ```
///
/// 0 = close at the period's absolute low; 100 = close at the period's absolute high.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen, or if the
/// high-low range is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NormalizedPrice;
/// use fin_primitives::signals::Signal;
///
/// let np = NormalizedPrice::new("np20", 20).unwrap();
/// assert_eq!(np.period(), 20);
/// ```
pub struct NormalizedPrice {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl NormalizedPrice {
    /// Constructs a new `NormalizedPrice`.
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
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for NormalizedPrice {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let highest = self.highs.iter().copied().reduce(Decimal::max).unwrap_or(bar.high);
        let lowest = self.lows.iter().copied().reduce(Decimal::min).unwrap_or(bar.low);
        let range = highest - lowest;

        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let np = (bar.close - lowest)
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::from(100u32);

        Ok(SignalValue::Scalar(np))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
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
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_np_invalid_period() {
        assert!(NormalizedPrice::new("np", 0).is_err());
    }

    #[test]
    fn test_np_unavailable_before_period() {
        let mut np = NormalizedPrice::new("np", 3).unwrap();
        assert_eq!(np.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert!(!np.is_ready());
    }

    #[test]
    fn test_np_close_at_top_equals_100() {
        // period=2: highest_high=110, lowest_low=90, close=110 => NP=100
        let mut np = NormalizedPrice::new("np", 2).unwrap();
        np.update_bar(&bar("105", "90", "95")).unwrap();
        let v = np.update_bar(&bar("110", "95", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_np_close_at_bottom_equals_0() {
        // close=90, highest=110, lowest=90 => NP=0
        let mut np = NormalizedPrice::new("np", 2).unwrap();
        np.update_bar(&bar("110", "95", "100")).unwrap();
        let v = np.update_bar(&bar("105", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_np_midpoint_equals_50() {
        // close=(90+110)/2=100, highest=110, lowest=90 => NP=50
        let mut np = NormalizedPrice::new("np", 2).unwrap();
        np.update_bar(&bar("110", "90", "100")).unwrap();
        let v = np.update_bar(&bar("108", "92", "100")).unwrap();
        if let SignalValue::Scalar(val) = v {
            assert_eq!(val, dec!(50));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_np_reset() {
        let mut np = NormalizedPrice::new("np", 2).unwrap();
        np.update_bar(&bar("110", "90", "100")).unwrap();
        np.update_bar(&bar("112", "88", "100")).unwrap();
        assert!(np.is_ready());
        np.reset();
        assert!(!np.is_ready());
    }
}
