//! Spinning Top candlestick pattern detector.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Spinning Top candlestick pattern detector.
///
/// A Spinning Top has a small body and long wicks on both sides, indicating
/// indecision and balance between buyers and sellers. Often appears at trend
/// turning points.
///
/// Criteria:
/// - Body size ≤ `max_body_pct` of range.
/// - Both upper and lower wicks ≥ `min_wick_pct` of range.
///
/// Output:
/// - `1.0` — Spinning Top detected (bullish body, close > open).
/// - `-1.0` — Spinning Top detected (bearish body, close < open).
/// - `0.5` — Spinning Top with doji body (close == open).
/// - `0.0` — No pattern.
///
/// This indicator is always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::SpinningTop;
/// use fin_primitives::signals::Signal;
/// let st = SpinningTop::new("st", 30, 20).unwrap();
/// assert!(st.is_ready());
/// ```
pub struct SpinningTop {
    name: String,
    max_body_pct: Decimal,
    min_wick_pct: Decimal,
}

impl SpinningTop {
    /// Constructs a new `SpinningTop` detector.
    ///
    /// - `max_body_pct`: maximum body size as % of range. Typical: 30.
    /// - `min_wick_pct`: minimum wick size as % of range for each wick. Typical: 20.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidInput`] if percentages are out of range.
    pub fn new(
        name: impl Into<String>,
        max_body_pct: u32,
        min_wick_pct: u32,
    ) -> Result<Self, FinError> {
        if max_body_pct > 100 || min_wick_pct > 100 {
            return Err(FinError::InvalidInput("percentage out of range".into()));
        }
        Ok(Self {
            name: name.into(),
            max_body_pct: Decimal::from(max_body_pct),
            min_wick_pct: Decimal::from(min_wick_pct),
        })
    }
}

impl Signal for SpinningTop {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let hundred = Decimal::from(100u32);
        let body = (bar.close - bar.open).abs();
        let body_high = bar.close.max(bar.open);
        let body_low = bar.close.min(bar.open);
        let upper_wick = bar.high - body_high;
        let lower_wick = body_low - bar.low;

        let body_pct = body
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(hundred)
            .ok_or(FinError::ArithmeticOverflow)?;
        let upper_pct = upper_wick
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(hundred)
            .ok_or(FinError::ArithmeticOverflow)?;
        let lower_pct = lower_wick
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(hundred)
            .ok_or(FinError::ArithmeticOverflow)?;

        if body_pct <= self.max_body_pct
            && upper_pct >= self.min_wick_pct
            && lower_pct >= self.min_wick_pct
        {
            let direction = if bar.close > bar.open {
                Decimal::ONE
            } else if bar.close < bar.open {
                Decimal::NEGATIVE_ONE
            } else {
                // Doji / neutral
                Decimal::new(5, 1) // 0.5
            };
            Ok(SignalValue::Scalar(direction))
        } else {
            Ok(SignalValue::Scalar(Decimal::ZERO))
        }
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn period(&self) -> usize {
        1
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
        assert!(SpinningTop::new("st", 101, 20).is_err());
        assert!(SpinningTop::new("st", 30, 101).is_err());
    }

    #[test]
    fn test_always_ready() {
        let st = SpinningTop::new("st", 30, 20).unwrap();
        assert!(st.is_ready());
    }

    #[test]
    fn test_bullish_spinning_top() {
        let mut st = SpinningTop::new("st", 30, 20).unwrap();
        // Range=10: body=1(10%), upper=4(40%), lower=5(50%) — body small, both wicks big
        // open=15, close=16, high=20, low=10
        let v = st.update_bar(&bar("15", "20", "10", "16")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bearish_spinning_top() {
        let mut st = SpinningTop::new("st", 30, 20).unwrap();
        // open=16, close=15, high=20, low=10
        let v = st.update_bar(&bar("16", "20", "10", "15")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_marubozu_not_spinning_top() {
        let mut st = SpinningTop::new("st", 30, 20).unwrap();
        // Full body marubozu — large body
        let v = st.update_bar(&bar("10", "20", "10", "20")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_one_sided_wick_not_spinning_top() {
        let mut st = SpinningTop::new("st", 30, 20).unwrap();
        // Long upper wick, no lower wick (hammer-like)
        // open=10, close=11, high=20, low=10 → lower_wick=0%
        let v = st.update_bar(&bar("10", "20", "10", "11")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_flat_bar_zero() {
        let mut st = SpinningTop::new("st", 30, 20).unwrap();
        let v = st.update_bar(&bar("10", "10", "10", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
