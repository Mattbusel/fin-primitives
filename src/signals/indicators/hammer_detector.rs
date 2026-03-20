//! Hammer / Shooting Star candlestick pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Hammer / Shooting Star detector.
///
/// Classifies each bar as a hammer, shooting star, or neither based on the
/// relationship between the body, upper wick, and lower wick relative to the
/// total bar range.
///
/// **Hammer** (bullish reversal candle): small body near the top of the bar,
/// long lower wick, minimal upper wick. Conditions:
/// - `lower_wick >= body * wick_ratio` (long lower shadow)
/// - `upper_wick <= body * 0.5` (minimal upper shadow)
/// - `range > 0`
///
/// **Shooting Star** (bearish reversal candle): small body near the bottom of
/// the bar, long upper wick, minimal lower wick. Conditions:
/// - `upper_wick >= body * wick_ratio` (long upper shadow)
/// - `lower_wick <= body * 0.5` (minimal lower shadow)
/// - `range > 0`
///
/// Output:
/// - **+1**: hammer pattern detected.
/// - **-1**: shooting star pattern detected.
/// - **0**: neither pattern.
///
/// Returns [`SignalValue::Unavailable`] when the bar range is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HammerDetector;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
/// let hd = HammerDetector::new("hd", dec!(2)).unwrap();
/// assert_eq!(hd.period(), 1);
/// ```
pub struct HammerDetector {
    name: String,
    /// Minimum ratio of wick to body to qualify as a hammer/shooting star.
    wick_ratio: Decimal,
}

impl HammerDetector {
    /// Constructs a new `HammerDetector`.
    ///
    /// `wick_ratio` controls how much larger the qualifying wick must be relative
    /// to the body. Common values: `2.0` (wick is 2× the body) or `1.5`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `wick_ratio <= 0`.
    pub fn new(name: impl Into<String>, wick_ratio: Decimal) -> Result<Self, FinError> {
        if wick_ratio <= Decimal::ZERO {
            return Err(FinError::InvalidInput("wick_ratio must be positive".into()));
        }
        Ok(Self { name: name.into(), wick_ratio })
    }

    /// Returns the wick-to-body ratio threshold.
    pub fn wick_ratio(&self) -> Decimal {
        self.wick_ratio
    }
}

impl Signal for HammerDetector {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let body = (bar.close - bar.open).abs();
        let upper_wick = bar.high - bar.open.max(bar.close);
        let lower_wick = bar.open.min(bar.close) - bar.low;

        // For doji (body=0), use a minimal body to avoid division by zero
        let effective_body = if body.is_zero() { range / Decimal::from(10u32) } else { body };

        let hammer = lower_wick >= effective_body * self.wick_ratio
            && upper_wick <= effective_body / Decimal::TWO;

        let shooting_star = upper_wick >= effective_body * self.wick_ratio
            && lower_wick <= effective_body / Decimal::TWO;

        let signal = if hammer {
            Decimal::ONE
        } else if shooting_star {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::ZERO
        };

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
    fn test_hd_invalid_wick_ratio() {
        assert!(HammerDetector::new("hd", dec!(0)).is_err());
        assert!(HammerDetector::new("hd", dec!(-1)).is_err());
    }

    #[test]
    fn test_hd_flat_bar_unavailable() {
        let mut hd = HammerDetector::new("hd", dec!(2)).unwrap();
        assert_eq!(hd.update_bar(&bar("100", "100", "100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_hd_hammer_detected() {
        let mut hd = HammerDetector::new("hd", dec!(2)).unwrap();
        // open=108, high=110, low=90, close=109
        // body = |109-108| = 1, upper_wick = 110-109 = 1, lower_wick = 108-90 = 18
        // lower_wick(18) >= body(1) * 2 = 2 ✓ and upper_wick(1) <= body(1)/2 = 0.5 ✗
        // Actually upper_wick=1 > 0.5, so NOT a hammer by strict criteria
        // Use: open=109, high=110, low=90, close=109 → body=0 → doji hammer
        // Or: open=109, high=110, low=90, close=110 → body=1, upper=0, lower=19
        // lower(19) >= 1*2=2 ✓, upper(0) <= 0.5 ✓ → hammer!
        let v = hd.update_bar(&bar("109", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_hd_shooting_star_detected() {
        let mut hd = HammerDetector::new("hd", dec!(2)).unwrap();
        // open=91, high=110, low=90, close=91 → body=0 (or small)
        // better: open=91, high=110, low=90, close=90 → body=1, upper=19, lower=0
        // upper(19) >= 1*2=2 ✓, lower(0) <= 0.5 ✓ → shooting star!
        let v = hd.update_bar(&bar("91", "110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_hd_neutral_bar_gives_zero() {
        let mut hd = HammerDetector::new("hd", dec!(2)).unwrap();
        // Balanced bar: open=95, high=110, low=90, close=105
        // body=10, upper=5, lower=5
        // lower(5) >= 10*2=20? No → not hammer
        // upper(5) >= 10*2=20? No → not shooting star
        let v = hd.update_bar(&bar("95", "110", "90", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hd_always_ready() {
        let hd = HammerDetector::new("hd", dec!(2)).unwrap();
        assert!(hd.is_ready());
    }

    #[test]
    fn test_hd_period_and_name() {
        let hd = HammerDetector::new("my_hd", dec!(2)).unwrap();
        assert_eq!(hd.period(), 1);
        assert_eq!(hd.name(), "my_hd");
        assert_eq!(hd.wick_ratio(), dec!(2));
    }
}
