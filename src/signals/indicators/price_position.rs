//! Price Position indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Position — where the current close sits within the rolling high-low range,
/// expressed as a percentage (0 = at the period low, 100 = at the period high).
///
/// ```text
/// price_position = (close - min_low(period)) / (max_high(period) - min_low(period)) × 100
/// ```
///
/// Similar to %K Stochastic but uses the absolute high/low range rather than the
/// candle H/L separately, making it smoother in trending markets.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or range is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PricePosition;
/// use fin_primitives::signals::Signal;
///
/// let pp = PricePosition::new("pp", 14).unwrap();
/// assert_eq!(pp.period(), 14);
/// ```
pub struct PricePosition {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl PricePosition {
    /// Creates a new `PricePosition`.
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

impl Signal for PricePosition {
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
        let range = max_high - min_low;

        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let pos = (bar.close - min_low)
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::from(100u32);

        Ok(SignalValue::Scalar(pos))
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(l.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pp_invalid_period() {
        assert!(PricePosition::new("p", 0).is_err());
    }

    #[test]
    fn test_pp_unavailable_before_period() {
        let mut pp = PricePosition::new("p", 3).unwrap();
        assert_eq!(pp.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pp_at_top_is_100() {
        // 3 bars: range 90-110, close at top = 110
        let mut pp = PricePosition::new("p", 3).unwrap();
        pp.update_bar(&bar("105", "90", "100")).unwrap();
        pp.update_bar(&bar("108", "91", "100")).unwrap();
        if let SignalValue::Scalar(v) = pp.update_bar(&bar("110", "92", "110")).unwrap() {
            // max_high = 110, min_low = 90, range = 20, close=110 → 100%
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pp_at_bottom_is_zero() {
        let mut pp = PricePosition::new("p", 3).unwrap();
        pp.update_bar(&bar("110", "90", "100")).unwrap();
        pp.update_bar(&bar("108", "92", "100")).unwrap();
        if let SignalValue::Scalar(v) = pp.update_bar(&bar("107", "90", "90")).unwrap() {
            // close = min_low = 90 → 0%
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_pp_flat_unavailable() {
        let mut pp = PricePosition::new("p", 2).unwrap();
        pp.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(pp.update_bar(&bar("100", "100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pp_reset() {
        let mut pp = PricePosition::new("p", 2).unwrap();
        pp.update_bar(&bar("110", "90", "100")).unwrap();
        pp.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(pp.is_ready());
        pp.reset();
        assert!(!pp.is_ready());
    }
}
