//! Engulfing Pattern indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Engulfing Pattern — detects bullish and bearish engulfing candlestick patterns.
///
/// A **bullish engulfing** occurs when:
/// - Previous bar is bearish (`prev_close < prev_open`)
/// - Current bar is bullish (`close > open`)
/// - Current bar's body fully engulfs the prior bar's body
///   (`open <= prev_close` and `close >= prev_open`)
///
/// A **bearish engulfing** is the mirror: prior bullish, current bearish, full engulf.
///
/// Outputs:
/// - `+1` → bullish engulfing
/// - `-1` → bearish engulfing
/// - `0` → no pattern
///
/// Returns [`SignalValue::Unavailable`] on the first bar (requires prior bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EngulfingPattern;
/// use fin_primitives::signals::Signal;
///
/// let ep = EngulfingPattern::new("ep").unwrap();
/// assert_eq!(ep.period(), 2);
/// ```
pub struct EngulfingPattern {
    name: String,
    prev_open: Option<Decimal>,
    prev_close: Option<Decimal>,
}

impl EngulfingPattern {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_open: None, prev_close: None })
    }
}

impl Signal for EngulfingPattern {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 2 }
    fn is_ready(&self) -> bool { self.prev_open.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match (self.prev_open, self.prev_close) {
            (Some(po), Some(pc)) => {
                let prev_bearish = pc < po;
                let prev_bullish = pc > po;
                let curr_bullish = bar.close > bar.open;
                let curr_bearish = bar.close < bar.open;

                if prev_bearish && curr_bullish
                    && bar.open <= pc && bar.close >= po
                {
                    SignalValue::Scalar(Decimal::ONE)
                } else if prev_bullish && curr_bearish
                    && bar.open >= pc && bar.close <= po
                {
                    SignalValue::Scalar(Decimal::NEGATIVE_ONE)
                } else {
                    SignalValue::Scalar(Decimal::ZERO)
                }
            }
            _ => SignalValue::Unavailable,
        };
        self.prev_open  = Some(bar.open);
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_open  = None;
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
    fn test_ep_first_bar_unavailable() {
        let mut ep = EngulfingPattern::new("ep").unwrap();
        assert_eq!(ep.update_bar(&bar("105","110","95","100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ep_bullish_engulfing() {
        let mut ep = EngulfingPattern::new("ep").unwrap();
        // Prior: bearish o=105, c=95
        ep.update_bar(&bar("105","110","90","95")).unwrap();
        // Current: bullish, open=94 <= prev_close=95, close=106 >= prev_open=105
        let r = ep.update_bar(&bar("94","110","90","106")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ep_bearish_engulfing() {
        let mut ep = EngulfingPattern::new("ep").unwrap();
        // Prior: bullish o=95, c=105
        ep.update_bar(&bar("95","110","90","105")).unwrap();
        // Current: bearish, open=106 >= prev_close=105, close=94 <= prev_open=95
        let r = ep.update_bar(&bar("106","110","90","94")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_ep_no_pattern() {
        let mut ep = EngulfingPattern::new("ep").unwrap();
        ep.update_bar(&bar("100","110","90","105")).unwrap();
        let r = ep.update_bar(&bar("103","110","90","107")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ep_reset() {
        let mut ep = EngulfingPattern::new("ep").unwrap();
        ep.update_bar(&bar("100","110","90","95")).unwrap();
        assert!(ep.is_ready());
        ep.reset();
        assert!(!ep.is_ready());
    }
}
