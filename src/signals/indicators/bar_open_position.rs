//! Bar Open Position — where the open falls within the bar's range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Bar Open Position — `(open - low) / (high - low)`.
///
/// Measures where the bar's open price falls within the bar's total range:
/// - **1.0**: open equals the high (opened at the top — often bearish reversal signal).
/// - **0.0**: open equals the low (opened at the bottom — often bullish reversal signal).
/// - **0.5**: open at the midpoint.
///
/// Returns [`SignalValue::Unavailable`] when the bar range is zero (flat bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarOpenPosition;
/// use fin_primitives::signals::Signal;
/// let bop = BarOpenPosition::new("bop");
/// assert_eq!(bop.period(), 1);
/// ```
pub struct BarOpenPosition {
    name: String,
}

impl BarOpenPosition {
    /// Constructs a new `BarOpenPosition`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for BarOpenPosition {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let pos = (bar.open - bar.low)
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(pos.clamp(Decimal::ZERO, Decimal::ONE)))
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
    fn test_bop_open_at_low_gives_zero() {
        let mut s = BarOpenPosition::new("bop");
        // open=low=90, high=110 → (90-90)/20 = 0
        let v = s.update_bar(&bar("90", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bop_open_at_high_gives_one() {
        let mut s = BarOpenPosition::new("bop");
        // open=high=110, low=90 → (110-90)/20 = 1
        let v = s.update_bar(&bar("110", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bop_open_at_midpoint() {
        let mut s = BarOpenPosition::new("bop");
        // open=100, high=110, low=90 → (100-90)/20 = 0.5
        let v = s.update_bar(&bar("100", "110", "90", "105")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!((r - dec!(0.5)).abs() < dec!(0.0001), "expected 0.5, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bop_flat_bar_unavailable() {
        let mut s = BarOpenPosition::new("bop");
        assert_eq!(s.update_bar(&bar("100", "100", "100", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bop_output_in_unit_interval() {
        let mut s = BarOpenPosition::new("bop");
        for (o, h, l, c) in &[("95","110","90","105"), ("108","115","102","110")] {
            if let SignalValue::Scalar(v) = s.update_bar(&bar(o, h, l, c)).unwrap() {
                assert!(v >= dec!(0) && v <= dec!(1), "out of [0,1]: {v}");
            }
        }
    }

    #[test]
    fn test_bop_always_ready() {
        let s = BarOpenPosition::new("bop");
        assert!(s.is_ready());
        assert_eq!(s.period(), 1);
    }
}
