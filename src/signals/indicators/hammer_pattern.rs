//! Hammer Pattern indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Hammer Pattern — detects bullish hammer and bearish hanging man candlestick patterns.
///
/// A **hammer** (bullish reversal, output `+1`) has:
/// - Small body in the upper portion of the range
/// - Lower shadow ≥ 2× the body size
/// - Little or no upper shadow (upper shadow ≤ 30% of total range)
///
/// A **hanging man** (bearish reversal, output `-1`) has the same shape but
/// occurs in an uptrend context. Since this indicator is stateless (no trend
/// context), it outputs `+1` for any hammer-shaped bar and `-1` if inverted
/// (small body in lower portion, long upper shadow).
///
/// Outputs `0` if no pattern is detected.
///
/// Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HammerPattern;
/// use fin_primitives::signals::Signal;
///
/// let hp = HammerPattern::new("hp").unwrap();
/// assert_eq!(hp.period(), 1);
/// ```
pub struct HammerPattern {
    name: String,
}

impl HammerPattern {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for HammerPattern {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let high  = bar.high;
        let low   = bar.low;
        let open  = bar.open;
        let close = bar.close;
        let range = high - low;

        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let body    = (close - open).abs();
        let body_hi = open.max(close);
        let body_lo = open.min(close);
        let upper_shadow = high - body_hi;
        let lower_shadow = body_lo - low;

        let two  = Decimal::TWO;
        let ratio_30 = Decimal::from_str_exact("0.30").unwrap_or(Decimal::ZERO);

        // Hammer: long lower shadow, small upper shadow
        let is_hammer = lower_shadow >= two * body
            && upper_shadow <= ratio_30 * range;

        // Inverted hammer / shooting star: long upper shadow, small lower shadow
        let is_inverted = upper_shadow >= two * body
            && lower_shadow <= ratio_30 * range;

        let signal = if is_hammer { Decimal::ONE }
            else if is_inverted { Decimal::NEGATIVE_ONE }
            else { Decimal::ZERO };
        Ok(SignalValue::Scalar(signal))
    }

    fn reset(&mut self) {}
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
    fn test_hp_always_ready() {
        let hp = HammerPattern::new("hp").unwrap();
        assert!(hp.is_ready());
    }

    #[test]
    fn test_hp_detects_hammer() {
        // open=98, high=100, low=80, close=99
        // body=1, range=20, lower=18, upper=1
        // lower(18) >= 2*body(2) ✓, upper(1) <= 30% of 20(6) ✓
        let mut hp = HammerPattern::new("hp").unwrap();
        let r = hp.update_bar(&bar("98","100","80","99")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_hp_detects_inverted_hammer() {
        // open=100, high=120, low=99, close=101
        // body=1, range=21, upper=19, lower=1
        // upper(19) >= 2*body(2) ✓, lower(1) <= 30% of 21(6.3) ✓
        let mut hp = HammerPattern::new("hp").unwrap();
        let r = hp.update_bar(&bar("100","120","99","101")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_hp_no_pattern_full_body() {
        // Marubozu (all body, no shadows)
        let mut hp = HammerPattern::new("hp").unwrap();
        let r = hp.update_bar(&bar("90","110","90","110")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hp_reset_is_noop() {
        let mut hp = HammerPattern::new("hp").unwrap();
        hp.reset(); // no state to reset
        assert!(hp.is_ready());
    }
}
