//! Volume Price Impact — price movement per unit of volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Volume Price Impact — measures how much price moved per unit of traded volume.
///
/// Defined as `|close - open| / volume` on each bar. A higher value means
/// each unit of volume moved price more (less liquid, or more directional flow).
/// A lower value means large volume with little price change (absorption).
///
/// Returns [`SignalValue::Unavailable`] when volume is zero (no trades occurred).
/// Returns [`SignalValue::Scalar`] on every bar with non-zero volume, making this a
/// **period-1 indicator**.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumePriceImpact;
/// use fin_primitives::signals::Signal;
/// let vpi = VolumePriceImpact::new("vpi");
/// assert_eq!(vpi.period(), 1);
/// ```
pub struct VolumePriceImpact {
    name: String,
    ready: bool,
}

impl VolumePriceImpact {
    /// Constructs a new `VolumePriceImpact`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ready: false }
    }
}

impl Signal for VolumePriceImpact {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if bar.volume.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let body = (bar.close - bar.open).abs();
        let impact = body
            .checked_div(bar.volume)
            .ok_or(FinError::ArithmeticOverflow)?;
        self.ready = true;
        Ok(SignalValue::Scalar(impact))
    }

    fn reset(&mut self) {
        self.ready = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, close: &str, vol: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let h = if o.value() >= c.value() { o } else { c };
        let l = if o.value() <= c.value() { o } else { c };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o, high: h, low: l, close: c,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vpi_zero_volume_unavailable() {
        let mut vpi = VolumePriceImpact::new("vpi");
        let v = vpi.update_bar(&bar("100", "105", "0")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
        assert!(!vpi.is_ready());
    }

    #[test]
    fn test_vpi_computes_correctly() {
        let mut vpi = VolumePriceImpact::new("vpi");
        // body = |110 - 100| = 10, volume = 100, impact = 0.1
        let v = vpi.update_bar(&bar("100", "110", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.1)));
        assert!(vpi.is_ready());
    }

    #[test]
    fn test_vpi_doji_gives_zero() {
        let mut vpi = VolumePriceImpact::new("vpi");
        // close = open → body = 0 → impact = 0
        let v = vpi.update_bar(&bar("100", "100", "500")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vpi_period_is_one() {
        let vpi = VolumePriceImpact::new("vpi");
        assert_eq!(vpi.period(), 1);
    }

    #[test]
    fn test_vpi_reset() {
        let mut vpi = VolumePriceImpact::new("vpi");
        vpi.update_bar(&bar("100", "110", "100")).unwrap();
        assert!(vpi.is_ready());
        vpi.reset();
        assert!(!vpi.is_ready());
    }

    #[test]
    fn test_vpi_always_non_negative() {
        let mut vpi = VolumePriceImpact::new("vpi");
        // Bearish bar: body = |95 - 100| = 5, volume = 50, impact = 0.1
        let v = vpi.update_bar(&bar("100", "95", "50")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r >= dec!(0), "impact should be non-negative: {r}");
        }
    }
}
