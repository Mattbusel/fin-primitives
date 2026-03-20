//! Open Midpoint Deviation indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Open Midpoint Deviation — how far the open price deviates from the bar's midpoint,
/// normalized by the bar's range.
///
/// ```text
/// midpoint = (high + low) / 2
/// deviation = (open - midpoint) / range
/// ```
///
/// - **+0.5**: open at the high (opened at the top).
/// - **−0.5**: open at the low (opened at the bottom).
/// - **0**: open exactly at midpoint.
/// - Returns `0` when the bar has no range (`high == low`).
/// - Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenMidpointDeviation;
/// use fin_primitives::signals::Signal;
///
/// let omd = OpenMidpointDeviation::new("omd").unwrap();
/// assert_eq!(omd.period(), 1);
/// ```
pub struct OpenMidpointDeviation {
    name: String,
}

impl OpenMidpointDeviation {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for OpenMidpointDeviation {
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
        let deviation = (bar.open - midpoint) / range;
        Ok(SignalValue::Scalar(deviation))
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
    fn test_omd_always_ready() {
        let omd = OpenMidpointDeviation::new("omd").unwrap();
        assert!(omd.is_ready());
    }

    #[test]
    fn test_omd_no_range() {
        let mut omd = OpenMidpointDeviation::new("omd").unwrap();
        let result = omd.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_omd_open_at_high() {
        // open=110, high=110, low=90 → midpoint=100, range=20, dev=(110-100)/20=0.5
        let mut omd = OpenMidpointDeviation::new("omd").unwrap();
        let result = omd.update_bar(&bar("110", "110", "90", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_omd_open_at_low() {
        // open=90, high=110, low=90 → midpoint=100, range=20, dev=(90-100)/20=-0.5
        let mut omd = OpenMidpointDeviation::new("omd").unwrap();
        let result = omd.update_bar(&bar("90", "110", "90", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(-0.5)));
    }

    #[test]
    fn test_omd_open_at_midpoint() {
        // open=100, high=110, low=90 → midpoint=100, deviation=0
        let mut omd = OpenMidpointDeviation::new("omd").unwrap();
        let result = omd.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }
}
