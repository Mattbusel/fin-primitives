//! Price Symmetry — measures intrabar midpoint deviation from open relative to range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Price Symmetry — `((high + low) / 2 - open) / (high - low)` in `[-0.5, 0.5]`.
///
/// Measures how symmetrically the bar's range is centered around the open:
/// - **Positive**: high-low midpoint is above the open — more upside range explored.
/// - **Zero**: open is exactly at the midpoint — symmetric bar.
/// - **Negative**: high-low midpoint is below the open — more downside range explored.
///
/// Returns `0` when `high == low` (zero range bar).
/// Always ready after first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceSymmetry;
/// use fin_primitives::signals::Signal;
/// let ps = PriceSymmetry::new("ps");
/// assert_eq!(ps.period(), 1);
/// ```
pub struct PriceSymmetry {
    name: String,
}

impl PriceSymmetry {
    /// Constructs a new `PriceSymmetry`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for PriceSymmetry {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let mid = (bar.high + bar.low)
            .checked_div(Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;
        let sym = (mid - bar.open)
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(sym))
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
    fn test_ps_always_ready() {
        let s = PriceSymmetry::new("ps");
        assert!(s.is_ready());
    }

    #[test]
    fn test_ps_symmetric_bar_gives_zero() {
        // Open=100, H=110, L=90 → mid=100, (100-100)/20 = 0
        let mut s = PriceSymmetry::new("ps");
        let v = s.update_bar(&bar("100","110","90","100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ps_open_at_low_gives_half() {
        // Open=90, H=110, L=90 → mid=100, (100-90)/20 = 0.5
        let mut s = PriceSymmetry::new("ps");
        if let SignalValue::Scalar(v) = s.update_bar(&bar("90","110","90","100")).unwrap() {
            assert!((v - dec!(0.5)).abs() < dec!(0.001), "open at low gives 0.5: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ps_open_at_high_gives_neg_half() {
        // Open=110, H=110, L=90 → mid=100, (100-110)/20 = -0.5
        let mut s = PriceSymmetry::new("ps");
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110","110","90","100")).unwrap() {
            assert!((v - dec!(-0.5)).abs() < dec!(0.001), "open at high gives -0.5: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ps_zero_range_gives_zero() {
        let mut s = PriceSymmetry::new("ps");
        let v = s.update_bar(&bar("100","100","100","100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ps_reset_noop() {
        let mut s = PriceSymmetry::new("ps");
        s.reset();
        assert!(s.is_ready());
    }
}
