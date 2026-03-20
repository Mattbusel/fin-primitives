//! Close-Above-Open ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close-Above-Open Ratio — percentage of bars in the last `period` where close > open.
///
/// Acts as a rolling bullish-bar dominance gauge. Values near 100 indicate a
/// strongly bullish run; values near 0 indicate a bearish run; ~50 indicates
/// a balanced/choppy market.
///
/// ```text
/// score[t] = (count of bars in window where close > open) / period × 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAboveOpen;
/// use fin_primitives::signals::Signal;
///
/// let mut cao = CloseAboveOpen::new("cao5", 5).unwrap();
/// assert_eq!(cao.period(), 5);
/// ```
pub struct CloseAboveOpen {
    name: String,
    period: usize,
    /// Stores 1 (close > open) or 0 for each bar in the rolling window.
    window: VecDeque<u8>,
    bull_count: usize,
}

impl CloseAboveOpen {
    /// Constructs a new `CloseAboveOpen`.
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
            bull_count: 0,
        })
    }
}

impl Signal for CloseAboveOpen {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let bull: u8 = if bar.close > bar.open { 1 } else { 0 };
        self.window.push_back(bull);
        self.bull_count += bull as usize;

        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.bull_count -= old as usize;
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        #[allow(clippy::cast_possible_truncation)]
        let ratio = Decimal::from(self.bull_count as u32)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::ONE_HUNDRED;

        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.window.clear();
        self.bull_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, close: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let high = if c.value() > o.value() { c } else { o };
        let low  = if c.value() < o.value() { c } else { o };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o, high, low, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cao_period_0_error() {
        assert!(CloseAboveOpen::new("cao", 0).is_err());
    }

    #[test]
    fn test_cao_unavailable_before_period() {
        let mut cao = CloseAboveOpen::new("cao3", 3).unwrap();
        assert_eq!(cao.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!cao.is_ready());
    }

    #[test]
    fn test_cao_all_bullish_is_100() {
        let mut cao = CloseAboveOpen::new("cao3", 3).unwrap();
        for _ in 0..3 {
            cao.update_bar(&bar("100", "105")).unwrap();
        }
        assert_eq!(cao.update_bar(&bar("100", "110")).unwrap(), SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_cao_all_bearish_is_0() {
        let mut cao = CloseAboveOpen::new("cao3", 3).unwrap();
        // Need 3 in window (period=3 means first scalar after 3rd push)
        cao.update_bar(&bar("105", "100")).unwrap();
        cao.update_bar(&bar("105", "100")).unwrap();
        let v = cao.update_bar(&bar("105", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cao_mixed_50_pct() {
        let mut cao = CloseAboveOpen::new("cao4", 4).unwrap();
        cao.update_bar(&bar("100", "105")).unwrap(); // bull
        cao.update_bar(&bar("105", "100")).unwrap(); // bear
        cao.update_bar(&bar("100", "105")).unwrap(); // bull
        let v = cao.update_bar(&bar("105", "100")).unwrap(); // bear  → 2/4 = 50%
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_cao_rolls_window() {
        let mut cao = CloseAboveOpen::new("cao3", 3).unwrap();
        cao.update_bar(&bar("100", "105")).unwrap(); // bull
        cao.update_bar(&bar("105", "100")).unwrap(); // bear
        cao.update_bar(&bar("100", "105")).unwrap(); // bull → 2/3 = 66.666...
        // Now add a bear bar — oldest (bull) rolls out, window = [bear, bull, bear] → 1/3
        let v = cao.update_bar(&bar("105", "100")).unwrap();
        let expected = dec!(100) / dec!(3);
        let diff = (v.as_decimal().unwrap() - expected).abs();
        assert!(diff < dec!(0.001), "expected ~33.33, got {:?}", v);
    }

    #[test]
    fn test_cao_reset() {
        let mut cao = CloseAboveOpen::new("cao3", 3).unwrap();
        for _ in 0..3 {
            cao.update_bar(&bar("100", "105")).unwrap();
        }
        assert!(cao.is_ready());
        cao.reset();
        assert!(!cao.is_ready());
        assert_eq!(cao.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
    }
}
