//! Trend Persistence indicator — fraction of bullish candles in a rolling window.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Persistence — measures directional consistency of recent price action.
///
/// Returns the fraction of bars in the last `period` bars where `close > open` (bullish).
///
/// - `1.0`: all `period` bars were bullish.
/// - `0.5`: equal bullish and bearish bars (no directional bias).
/// - `0.0`: all `period` bars were bearish.
///
/// Doji bars (close == open) are counted as neither bullish nor bearish; they
/// contribute to the denominator but not the numerator. The result remains in `[0, 1]`.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendPersistence;
/// use fin_primitives::signals::Signal;
/// let tp = TrendPersistence::new("tp_10", 10).unwrap();
/// assert_eq!(tp.period(), 10);
/// ```
pub struct TrendPersistence {
    name: String,
    period: usize,
    /// Rolling window: `true` = bullish bar, `false` = not bullish.
    window: VecDeque<bool>,
}

impl TrendPersistence {
    /// Constructs a new `TrendPersistence`.
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
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for TrendPersistence {
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
        self.window.push_back(bar.is_bullish());
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let bullish_count = self.window.iter().filter(|&&b| b).count();
        #[allow(clippy::cast_possible_truncation)]
        let ratio = Decimal::from(bullish_count as u32)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
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

    fn bull() -> OhlcvBar {
        make_bar("100", "110")
    }

    fn bear() -> OhlcvBar {
        make_bar("110", "100")
    }

    fn make_bar(open: &str, close: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let h = if o.value() > c.value() { o } else { c };
        let l = if o.value() < c.value() { o } else { c };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o, high: h, low: l, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_tp_invalid_period() {
        assert!(TrendPersistence::new("tp", 0).is_err());
    }

    #[test]
    fn test_tp_unavailable_before_period() {
        let mut tp = TrendPersistence::new("tp", 3).unwrap();
        assert_eq!(tp.update_bar(&bull()).unwrap(), SignalValue::Unavailable);
        assert_eq!(tp.update_bar(&bull()).unwrap(), SignalValue::Unavailable);
        assert!(!tp.is_ready());
    }

    #[test]
    fn test_tp_all_bullish_gives_one() {
        let mut tp = TrendPersistence::new("tp", 3).unwrap();
        tp.update_bar(&bull()).unwrap();
        tp.update_bar(&bull()).unwrap();
        let v = tp.update_bar(&bull()).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_tp_all_bearish_gives_zero() {
        let mut tp = TrendPersistence::new("tp", 3).unwrap();
        tp.update_bar(&bear()).unwrap();
        tp.update_bar(&bear()).unwrap();
        let v = tp.update_bar(&bear()).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_tp_half_bullish() {
        let mut tp = TrendPersistence::new("tp", 4).unwrap();
        tp.update_bar(&bull()).unwrap();
        tp.update_bar(&bull()).unwrap();
        tp.update_bar(&bear()).unwrap();
        let v = tp.update_bar(&bear()).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_tp_rolling_window() {
        let mut tp = TrendPersistence::new("tp", 2).unwrap();
        tp.update_bar(&bull()).unwrap();
        tp.update_bar(&bull()).unwrap();
        // Window: [bull, bull] → 1.0
        tp.update_bar(&bear()).unwrap();
        // Window: [bull, bear] → 0.5
        let v = tp.update_bar(&bear()).unwrap();
        // Window: [bear, bear] → 0.0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_tp_output_in_unit_interval() {
        let mut tp = TrendPersistence::new("tp", 5).unwrap();
        let bars = [bull(), bear(), bull(), bear(), bull(), bull(), bear()];
        for b in &bars {
            if let SignalValue::Scalar(v) = tp.update_bar(b).unwrap() {
                assert!(v >= dec!(0));
                assert!(v <= dec!(1));
            }
        }
    }

    #[test]
    fn test_tp_reset() {
        let mut tp = TrendPersistence::new("tp", 2).unwrap();
        tp.update_bar(&bull()).unwrap();
        tp.update_bar(&bull()).unwrap();
        assert!(tp.is_ready());
        tp.reset();
        assert!(!tp.is_ready());
    }

    #[test]
    fn test_tp_period_and_name() {
        let tp = TrendPersistence::new("my_tp", 14).unwrap();
        assert_eq!(tp.period(), 14);
        assert_eq!(tp.name(), "my_tp");
    }
}
