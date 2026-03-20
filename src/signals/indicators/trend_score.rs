//! Trend Score indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Score — percentage of bars in the window where close > previous close.
///
/// ```text
/// up_bars = count(close_t > close_{t−1}, period)
/// output  = up_bars / period × 100
/// ```
///
/// Values near 100 indicate a strong uptrend; near 0 a downtrend; near 50 choppy.
/// Simple and interpretable trend strength measure.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendScore;
/// use fin_primitives::signals::Signal;
///
/// let ts = TrendScore::new("ts", 10).unwrap();
/// assert_eq!(ts.period(), 10);
/// ```
pub struct TrendScore {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    up_flags: VecDeque<u8>,
}

impl TrendScore {
    /// Creates a new `TrendScore`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            up_flags: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for TrendScore {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let up = if bar.close > pc { 1u8 } else { 0u8 };
            self.up_flags.push_back(up);
            if self.up_flags.len() > self.period { self.up_flags.pop_front(); }
        }
        self.prev_close = Some(bar.close);

        if self.up_flags.len() < self.period { return Ok(SignalValue::Unavailable); }

        let up_count: u32 = self.up_flags.iter().map(|&f| f as u32).sum();
        let score = Decimal::from(up_count) / Decimal::from(self.period as u32) * Decimal::from(100u32);
        Ok(SignalValue::Scalar(score))
    }

    fn is_ready(&self) -> bool { self.up_flags.len() >= self.period }
    fn period(&self) -> usize { self.period }

    fn reset(&mut self) {
        self.prev_close = None;
        self.up_flags.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ts_invalid() {
        assert!(TrendScore::new("t", 0).is_err());
    }

    #[test]
    fn test_ts_unavailable_before_warmup() {
        let mut t = TrendScore::new("t", 3).unwrap();
        for _ in 0..3 {
            assert_eq!(t.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ts_all_up_is_100() {
        let mut t = TrendScore::new("t", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..7 {
            let p = format!("{}", 100 + i);
            last = t.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(100));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ts_all_down_is_zero() {
        let mut t = TrendScore::new("t", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for i in 0u32..7 {
            let p = format!("{}", 200 - i);
            last = t.update_bar(&bar(&p)).unwrap();
        }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ts_flat_is_zero() {
        // Flat: close never > prev_close → all flags = 0 → score = 0
        let mut t = TrendScore::new("t", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..7 { last = t.update_bar(&bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0));
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_ts_range_0_to_100() {
        let mut t = TrendScore::new("t", 4).unwrap();
        for price in ["100", "102", "101", "103", "102", "104", "100", "101"] {
            if let SignalValue::Scalar(v) = t.update_bar(&bar(price)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(100), "out of range: {v}");
            }
        }
    }

    #[test]
    fn test_ts_reset() {
        let mut t = TrendScore::new("t", 3).unwrap();
        for i in 0u32..7 {
            let p = format!("{}", 100 + i);
            t.update_bar(&bar(&p)).unwrap();
        }
        assert!(t.is_ready());
        t.reset();
        assert!(!t.is_ready());
    }
}
