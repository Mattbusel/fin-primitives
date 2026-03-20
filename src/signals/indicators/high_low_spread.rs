//! High-Low Spread Percent indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Low Spread Percent — rolling range width as a percentage of the lowest low.
///
/// ```text
/// spread_pct = (max_high(period) - min_low(period)) / min_low(period) × 100
/// ```
///
/// Measures how wide the price range has been over the lookback window.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen,
/// or when `min_low` is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowSpread;
/// use fin_primitives::signals::Signal;
///
/// let hl = HighLowSpread::new("hls", 14).unwrap();
/// assert_eq!(hl.period(), 14);
/// ```
pub struct HighLowSpread {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl HighLowSpread {
    /// Creates a new `HighLowSpread`.
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

impl Signal for HighLowSpread {
    fn name(&self) -> &str { &self.name }

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

        let max_high = self.highs.iter().copied().max().unwrap();
        let min_low = self.lows.iter().copied().min().unwrap();

        if min_low.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let spread = (max_high - min_low)
            .checked_div(min_low)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::from(100u32);

        Ok(SignalValue::Scalar(spread))
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
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
    fn test_hls_invalid_period() {
        assert!(HighLowSpread::new("h", 0).is_err());
    }

    #[test]
    fn test_hls_unavailable_before_period() {
        let mut hl = HighLowSpread::new("h", 3).unwrap();
        assert_eq!(hl.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert_eq!(hl.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_hls_flat_range_zero() {
        let mut hl = HighLowSpread::new("h", 2).unwrap();
        hl.update_bar(&bar("100", "100")).unwrap();
        if let SignalValue::Scalar(v) = hl.update_bar(&bar("100", "100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hls_known_value() {
        // max_high=110, min_low=90 => spread = 20/90*100 = 22.222...
        let mut hl = HighLowSpread::new("h", 2).unwrap();
        hl.update_bar(&bar("110", "90")).unwrap();
        if let SignalValue::Scalar(v) = hl.update_bar(&bar("105", "95")).unwrap() {
            // max_high=110, min_low=90 => 20/90*100
            let expected = dec!(20) / dec!(90) * dec!(100);
            assert_eq!(v, expected);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_hls_reset() {
        let mut hl = HighLowSpread::new("h", 2).unwrap();
        hl.update_bar(&bar("110", "90")).unwrap();
        hl.update_bar(&bar("110", "90")).unwrap();
        assert!(hl.is_ready());
        hl.reset();
        assert!(!hl.is_ready());
        assert_eq!(hl.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }
}
