//! Engulfing Candle detector — bullish and bearish engulfing patterns.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Engulfing Candle Detector — detects bullish and bearish engulfing patterns.
///
/// An engulfing pattern requires two consecutive bars where the current bar's body
/// fully contains the prior bar's body:
/// - **+1 (Bullish Engulfing)**: prior bar is bearish (`open > close`), current bar is bullish
///   (`close > open`), and `open <= prev_close` and `close >= prev_open`.
/// - **−1 (Bearish Engulfing)**: prior bar is bullish (`close > open`), current bar is bearish
///   (`open > close`), and `close <= prev_close` and `open >= prev_open`.
/// - **0**: no engulfing pattern.
///
/// Returns [`SignalValue::Unavailable`] until 2 bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EngulfingDetector;
/// use fin_primitives::signals::Signal;
/// let ed = EngulfingDetector::new("engulf");
/// assert_eq!(ed.period(), 1);
/// ```
pub struct EngulfingDetector {
    name: String,
    prev_open: Option<Decimal>,
    prev_close: Option<Decimal>,
}

impl EngulfingDetector {
    /// Constructs a new `EngulfingDetector`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_open: None, prev_close: None }
    }
}

impl Signal for EngulfingDetector {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_open.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let (Some(po), Some(pc)) = (self.prev_open, self.prev_close) {
            let prev_bearish = po > pc;
            let prev_bullish = pc > po;
            let curr_bullish = bar.close > bar.open;
            let curr_bearish = bar.open > bar.close;

            let signal = if prev_bearish && curr_bullish
                && bar.open <= pc
                && bar.close >= po
            {
                Decimal::ONE        // bullish engulfing
            } else if prev_bullish && curr_bearish
                && bar.close <= pc
                && bar.open >= po
            {
                Decimal::NEGATIVE_ONE   // bearish engulfing
            } else {
                Decimal::ZERO
            };
            Ok(SignalValue::Scalar(signal))
        } else {
            Ok(SignalValue::Unavailable)
        };

        self.prev_open = Some(bar.open);
        self.prev_close = Some(bar.close);
        result
    }

    fn reset(&mut self) {
        self.prev_open = None;
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ed_first_bar_unavailable() {
        let mut s = EngulfingDetector::new("ed");
        assert_eq!(s.update_bar(&bar("105", "110", "100", "102")).unwrap(), SignalValue::Unavailable);
        assert!(s.is_ready());
    }

    #[test]
    fn test_ed_bullish_engulfing() {
        let mut s = EngulfingDetector::new("ed");
        // Prior: bearish bar, open=105, close=100
        s.update_bar(&bar("105", "110", "98", "100")).unwrap();
        // Current: bullish, open<=100 (prev_close), close>=105 (prev_open)
        let v = s.update_bar(&bar("99", "112", "97", "107")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ed_bearish_engulfing() {
        let mut s = EngulfingDetector::new("ed");
        // Prior: bullish bar, open=100, close=105
        s.update_bar(&bar("100", "108", "98", "105")).unwrap();
        // Current: bearish, open>=105 (prev_close), close<=100 (prev_open)
        let v = s.update_bar(&bar("106", "109", "97", "99")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_ed_no_engulfing() {
        let mut s = EngulfingDetector::new("ed");
        // Two consecutive bullish bars — no engulfing
        s.update_bar(&bar("100", "108", "98", "105")).unwrap();
        let v = s.update_bar(&bar("106", "112", "104", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ed_reset() {
        let mut s = EngulfingDetector::new("ed");
        s.update_bar(&bar("100", "108", "98", "105")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
