//! Volume Direction Ratio indicator -- estimated buy/sell imbalance from price action.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume Direction Ratio -- approximates net buying pressure as a fraction of total volume.
///
/// Since intra-bar tick direction is unavailable at the OHLCV level, this indicator uses
/// the close position within the bar range as a proxy for buy vs. sell volume:
///
/// ```text
/// close_pct = (close - low) / (high - low)   (0 = all selling, 1 = all buying)
/// vdr[t]    = 2 * close_pct - 1              (rescaled to [-1, +1])
/// ```
///
/// A value of +1 means the close was at the high (fully bullish); -1 means at the low.
/// Returns [`SignalValue::Unavailable`] when `high == low` (zero-range bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeDirectionRatio;
/// use fin_primitives::signals::Signal;
/// let vdr = VolumeDirectionRatio::new("vdr");
/// assert_eq!(vdr.period(), 1);
/// ```
pub struct VolumeDirectionRatio {
    name: String,
}

impl VolumeDirectionRatio {
    /// Constructs a new `VolumeDirectionRatio`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for VolumeDirectionRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        let close_pct = (bar.close - bar.low) / range;
        let vdr = Decimal::from(2u32) * close_pct - Decimal::ONE;
        Ok(SignalValue::Scalar(vdr))
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
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vdr_close_at_high_is_plus_one() {
        let mut vdr = VolumeDirectionRatio::new("vdr");
        let v = vdr.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_vdr_close_at_low_is_minus_one() {
        let mut vdr = VolumeDirectionRatio::new("vdr");
        let v = vdr.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_vdr_close_at_midpoint_is_zero() {
        let mut vdr = VolumeDirectionRatio::new("vdr");
        let v = vdr.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vdr_zero_range_unavailable() {
        let mut vdr = VolumeDirectionRatio::new("vdr");
        let v = vdr.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_vdr_always_ready() {
        let vdr = VolumeDirectionRatio::new("vdr");
        assert!(vdr.is_ready());
    }
}
