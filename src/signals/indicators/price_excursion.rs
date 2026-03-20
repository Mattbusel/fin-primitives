//! Price Excursion indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Price Excursion — normalized adverse intrabar excursion against the bar's closing direction.
///
/// Measures how far price moved against the eventual close direction within the bar:
/// ```text
/// if close > open (bullish): excursion = (open - low) / range
/// if close < open (bearish): excursion = (high - open) / range
/// if close == open (doji):   excursion = 0.5 (symmetrically adverse)
/// ```
///
/// - **Near 1.0**: price dipped far against the closing direction before reversing.
/// - **Near 0.0**: price moved directly in the closing direction with minimal adverse move.
/// - Returns `0.5` when range is zero.
/// - Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceExcursion;
/// use fin_primitives::signals::Signal;
///
/// let pe = PriceExcursion::new("pe").unwrap();
/// assert_eq!(pe.period(), 1);
/// ```
pub struct PriceExcursion {
    name: String,
}

impl PriceExcursion {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for PriceExcursion {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::new(5, 1))); // 0.5
        }
        let excursion = if bar.close > bar.open {
            // Bullish — adverse = downside from open
            (bar.open - bar.low) / range
        } else if bar.close < bar.open {
            // Bearish — adverse = upside from open
            (bar.high - bar.open) / range
        } else {
            // Doji
            Decimal::new(5, 1)
        };
        Ok(SignalValue::Scalar(excursion))
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
    fn test_pe_always_ready() {
        let pe = PriceExcursion::new("pe").unwrap();
        assert!(pe.is_ready());
    }

    #[test]
    fn test_pe_no_range() {
        let mut pe = PriceExcursion::new("pe").unwrap();
        let result = pe.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_pe_bullish_no_adverse() {
        // Bullish bar: open=90, close=110, high=110, low=90 → (90-90)/20=0
        let mut pe = PriceExcursion::new("pe").unwrap();
        let result = pe.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pe_bullish_large_adverse() {
        // Bullish: open=100, close=105, high=110, low=90 → (100-90)/20=0.5
        let mut pe = PriceExcursion::new("pe").unwrap();
        let result = pe.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_pe_bearish_large_adverse() {
        // Bearish: open=100, close=95, high=110, low=90 → (110-100)/20=0.5
        let mut pe = PriceExcursion::new("pe").unwrap();
        let result = pe.update_bar(&bar("100", "110", "90", "95")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.5)));
    }
}
