//! Price Bandwidth indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Bandwidth — where the current close falls within the rolling `period`-bar
/// high-low channel, expressed as a value in [0, 1].
///
/// ```text
/// bandwidth = (close - rolling_low(n)) / (rolling_high(n) - rolling_low(n))
/// ```
///
/// - `1.0` → close at the rolling channel top  
/// - `0.5` → close at the channel midpoint  
/// - `0.0` → close at the rolling channel bottom  
///
/// Returns `0.5` if channel range is zero.
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceBandwidth;
/// use fin_primitives::signals::Signal;
///
/// let pb = PriceBandwidth::new("pb", 20).unwrap();
/// assert_eq!(pb.period(), 20);
/// ```
pub struct PriceBandwidth {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl PriceBandwidth {
    /// Constructs a new `PriceBandwidth`.
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

impl Signal for PriceBandwidth {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.highs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period { self.highs.pop_front(); }
        if self.lows.len() > self.period { self.lows.pop_front(); }

        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let max_h = self.highs.iter().copied().fold(self.highs[0], |acc, v| acc.max(v));
        let min_l = self.lows.iter().copied().fold(self.lows[0], |acc, v| acc.min(v));
        let channel = max_h - min_l;

        if channel.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::new(5, 1))); // 0.5
        }

        Ok(SignalValue::Scalar((bar.close - min_l) / channel))
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
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_pb_invalid_period() {
        assert!(PriceBandwidth::new("pb", 0).is_err());
    }

    #[test]
    fn test_pb_unavailable_before_warm_up() {
        let mut pb = PriceBandwidth::new("pb", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(pb.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_pb_close_at_channel_top() {
        let mut pb = PriceBandwidth::new("pb", 3).unwrap();
        // All bars: high=110, low=90, close=110 → bandwidth = (110-90)/(110-90) = 1
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = pb.update_bar(&bar("110", "90", "110")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_pb_close_at_channel_bottom() {
        let mut pb = PriceBandwidth::new("pb", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..3 {
            last = pb.update_bar(&bar("110", "90", "90")).unwrap();
        }
        assert_eq!(last, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pb_reset() {
        let mut pb = PriceBandwidth::new("pb", 3).unwrap();
        for _ in 0..3 { pb.update_bar(&bar("110", "90", "100")).unwrap(); }
        assert!(pb.is_ready());
        pb.reset();
        assert!(!pb.is_ready());
    }
}
