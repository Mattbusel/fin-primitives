//! Volume Per Range — volume divided by bar range (liquidity density measure).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Volume Per Range — `volume / (high - low)`.
///
/// Measures the volume traded per unit of price movement — a proxy for market
/// liquidity or order-flow density within the bar:
/// - **High value**: large volume relative to range (absorption / tight market).
/// - **Low value**: small volume relative to range (thin/volatile market).
///
/// Returns [`SignalValue::Unavailable`] when the bar range is zero (flat bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumePerRange;
/// use fin_primitives::signals::Signal;
/// let vpr = VolumePerRange::new("vpr");
/// assert_eq!(vpr.period(), 1);
/// ```
pub struct VolumePerRange {
    name: String,
}

impl VolumePerRange {
    /// Constructs a new `VolumePerRange`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for VolumePerRange {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let vpr = bar.volume
            .checked_div(range)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(vpr))
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

    fn bar(h: &str, l: &str, vol: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vpr_basic() {
        let mut s = VolumePerRange::new("vpr");
        // volume=2000, range=20 → 100
        let v = s.update_bar(&bar("110", "90", "2000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_vpr_flat_bar_unavailable() {
        let mut s = VolumePerRange::new("vpr");
        assert_eq!(s.update_bar(&bar("100", "100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vpr_non_negative() {
        let mut s = VolumePerRange::new("vpr");
        let bars = [
            bar("110", "90", "1000"),
            bar("108", "92", "2500"),
            bar("115", "85", "500"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = s.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "VPR must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_vpr_always_ready() {
        let s = VolumePerRange::new("vpr");
        assert!(s.is_ready());
        assert_eq!(s.period(), 1);
    }
}
