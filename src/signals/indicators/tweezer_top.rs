//! Tweezer Top candlestick pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Tweezer Top pattern detector.
///
/// The bearish counterpart of the Tweezer Bottom: two consecutive bars with
/// approximately equal highs at a resistance level, suggesting strong selling
/// pressure. A bearish reversal signal.
///
/// Criteria:
/// - The difference between the two highs is ≤ `tolerance_pct` of the average range.
/// - The second bar should be bearish (or neutral).
///
/// Output:
/// - `-1.0` — Tweezer Top detected.
/// - `0.0`  — No pattern.
///
/// Returns `SignalValue::Unavailable` on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TweezerTop;
/// use fin_primitives::signals::Signal;
/// let tt = TweezerTop::new("tt", 5).unwrap();
/// assert_eq!(tt.period(), 2);
/// ```
pub struct TweezerTop {
    name: String,
    tolerance_pct: Decimal,
    prev: Option<BarInput>,
}

impl TweezerTop {
    /// Constructs a new `TweezerTop` detector.
    ///
    /// `tolerance_pct`: maximum allowed difference between the two highs as a
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

impl Signal for TweezerTop {
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
                let high_diff = (bar.high - prev.high).abs();
                let diff_pct = high_diff
                    .checked_div(avg_range)
                    .ok_or(FinError::ArithmeticOverflow)?
                    .checked_mul(Decimal::from(100u32))
                    .ok_or(FinError::ArithmeticOverflow)?;

                if diff_pct <= self.tolerance_pct && bar.close <= bar.open {
                    SignalValue::Scalar(Decimal::NEGATIVE_ONE)
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
        assert!(TweezerTop::new("tt", 101).is_err());
    }

    #[test]
    fn test_first_bar_unavailable() {
        let mut tt = TweezerTop::new("tt", 5).unwrap();
        assert_eq!(tt.update_bar(&bar("15", "20", "14", "17")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_tweezer_top_exact_match() {
        let mut tt = TweezerTop::new("tt", 5).unwrap();
        // Both bars have high=20, second bar bearish
        tt.update_bar(&bar("15", "20", "14", "18")).unwrap();
        let v = tt.update_bar(&bar("18", "20", "14", "15")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_different_highs_no_pattern() {
        let mut tt = TweezerTop::new("tt", 5).unwrap();
        tt.update_bar(&bar("15", "20", "14", "18")).unwrap();
        // High=25, far from 20 — no tweezer
        let v = tt.update_bar(&bar("20", "25", "14", "15")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bullish_second_bar_no_pattern() {
        let mut tt = TweezerTop::new("tt", 5).unwrap();
        tt.update_bar(&bar("15", "20", "14", "18")).unwrap();
        // Same high but second bar is bullish
        let v = tt.update_bar(&bar("15", "20", "14", "19")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut tt = TweezerTop::new("tt", 5).unwrap();
        tt.update_bar(&bar("15", "20", "14", "18")).unwrap();
        assert!(tt.is_ready());
        tt.reset();
        assert!(!tt.is_ready());
    }
}
