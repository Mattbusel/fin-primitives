//! Tweezer Bottom candlestick pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Tweezer Bottom pattern detector.
///
/// Two consecutive bars with approximately equal lows at a support level,
/// suggesting strong buying pressure at that price. A bullish reversal signal.
///
/// Criteria:
/// - The difference between the two lows is ≤ `tolerance_pct` of the average range.
/// - The second bar should be bullish (or neutral).
///
/// Output:
/// - `1.0` — Tweezer Bottom detected.
/// - `0.0` — No pattern.
///
/// Returns `SignalValue::Unavailable` on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TweezerBottom;
/// use fin_primitives::signals::Signal;
/// let tb = TweezerBottom::new("tb", 5).unwrap();
/// assert_eq!(tb.period(), 2);
/// ```
pub struct TweezerBottom {
    name: String,
    tolerance_pct: Decimal,
    prev: Option<BarInput>,
}

impl TweezerBottom {
    /// Constructs a new `TweezerBottom` detector.
    ///
    /// `tolerance_pct`: maximum allowed difference between the two lows as a
    /// percentage of the average range (0–100). Typical: 5.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if `tolerance_pct > 100`.
    pub fn new(name: impl Into<String>, tolerance_pct: u32) -> Result<Self, FinError> {
        if tolerance_pct > 100 {
            return Err(FinError::InvalidInput("tolerance_pct out of range".into()));
        }
        Ok(Self {
            name: name.into(),
            tolerance_pct: Decimal::from(tolerance_pct),
            prev: None,
        })
    }
}

impl Signal for TweezerBottom {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(ref prev) = self.prev {
            let avg_range = (prev.high - prev.low + bar.high - bar.low)
                .checked_div(Decimal::from(2u32))
                .ok_or(FinError::ArithmeticOverflow)?;

            if avg_range.is_zero() {
                SignalValue::Scalar(Decimal::ZERO)
            } else {
                let low_diff = (bar.low - prev.low).abs();
                let diff_pct = low_diff
                    .checked_div(avg_range)
                    .ok_or(FinError::ArithmeticOverflow)?
                    .checked_mul(Decimal::from(100u32))
                    .ok_or(FinError::ArithmeticOverflow)?;

                if diff_pct <= self.tolerance_pct && bar.close >= bar.open {
                    SignalValue::Scalar(Decimal::ONE)
                } else {
                    SignalValue::Scalar(Decimal::ZERO)
                }
            }
        } else {
            SignalValue::Unavailable
        };

        self.prev = Some(*bar);
        Ok(result)
    }

    fn is_ready(&self) -> bool {
        self.prev.is_some()
    }

    fn period(&self) -> usize {
        2
    }

    fn reset(&mut self) {
        self.prev = None;
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
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
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
    fn test_invalid_pct_fails() {
        assert!(TweezerBottom::new("tb", 101).is_err());
    }

    #[test]
    fn test_first_bar_unavailable() {
        let mut tb = TweezerBottom::new("tb", 5).unwrap();
        assert_eq!(tb.update_bar(&bar("10", "15", "9", "12")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_tweezer_bottom_exact_match() {
        let mut tb = TweezerBottom::new("tb", 5).unwrap();
        // Both bars have low=9, second bar bullish
        tb.update_bar(&bar("12", "15", "9", "10")).unwrap();
        let v = tb.update_bar(&bar("9", "14", "9", "13")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_different_lows_no_pattern() {
        let mut tb = TweezerBottom::new("tb", 5).unwrap();
        tb.update_bar(&bar("12", "15", "9", "10")).unwrap();
        // Low=6, far from 9 — no tweezer
        let v = tb.update_bar(&bar("9", "14", "6", "12")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bearish_second_bar_no_pattern() {
        let mut tb = TweezerBottom::new("tb", 5).unwrap();
        tb.update_bar(&bar("12", "15", "9", "10")).unwrap();
        // Same low but second bar is bearish
        let v = tb.update_bar(&bar("13", "14", "9", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut tb = TweezerBottom::new("tb", 5).unwrap();
        tb.update_bar(&bar("12", "15", "9", "10")).unwrap();
        assert!(tb.is_ready());
        tb.reset();
        assert!(!tb.is_ready());
    }
}
