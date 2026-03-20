//! Midpoint Oscillator indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Midpoint Oscillator — measures how far the close is from the midpoint of
/// the bar's high-low range, expressed as a fraction of the range.
///
/// ```text
/// midpoint = (high + low) / 2
/// osc      = (close - midpoint) / (high - low)
/// ```
///
/// - `+0.5` → close at the high  
/// - `0` → close at the midpoint  
/// - `-0.5` → close at the low  
///
/// Returns `0` when the bar has no range (`high == low`).
/// Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MidpointOscillator;
/// use fin_primitives::signals::Signal;
///
/// let mo = MidpointOscillator::new("mo").unwrap();
/// assert_eq!(mo.period(), 1);
/// ```
pub struct MidpointOscillator {
    name: String,
}

impl MidpointOscillator {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for MidpointOscillator {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let two = Decimal::TWO;
        let midpoint = (bar.high + bar.low) / two;
        Ok(SignalValue::Scalar((bar.close - midpoint) / range))
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_mo_always_ready() {
        let mo = MidpointOscillator::new("mo").unwrap();
        assert!(mo.is_ready());
    }

    #[test]
    fn test_mo_no_range() {
        let mut mo = MidpointOscillator::new("mo").unwrap();
        assert_eq!(mo.update_bar(&bar("100", "100", "100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_mo_close_at_midpoint() {
        let mut mo = MidpointOscillator::new("mo").unwrap();
        // high=110, low=90, mid=100, close=100 → 0
        assert_eq!(mo.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_mo_close_at_high() {
        let mut mo = MidpointOscillator::new("mo").unwrap();
        // high=110, low=90, mid=100, close=110 → (110-100)/20 = 0.5
        assert_eq!(mo.update_bar(&bar("110", "90", "110")).unwrap(), SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_mo_close_at_low() {
        let mut mo = MidpointOscillator::new("mo").unwrap();
        // high=110, low=90, mid=100, close=90 → (90-100)/20 = -0.5
        assert_eq!(mo.update_bar(&bar("110", "90", "90")).unwrap(), SignalValue::Scalar(dec!(-0.5)));
    }
}
