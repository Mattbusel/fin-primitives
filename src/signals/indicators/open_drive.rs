//! Open Drive indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Open Drive — how far price moved away from the bar's open, normalized by the bar's range.
///
/// ```text
/// max_excursion = max(|high - open|, |low - open|)
/// open_drive    = max_excursion / range
/// ```
///
/// - **Near 1.0**: price made most of its range movement starting from the open
///   (strong directional opening drive).
/// - **Near 0.5**: balanced — price moved equally above and below the open.
/// - **Near 0.0**: open was near the high or low, and the range was entirely on one side.
/// - Returns `0.5` when range is zero (`high == low`).
/// - Always ready from the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenDrive;
/// use fin_primitives::signals::Signal;
///
/// let od = OpenDrive::new("od").unwrap();
/// assert_eq!(od.period(), 1);
/// ```
pub struct OpenDrive {
    name: String,
}

impl OpenDrive {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into() })
    }
}

impl Signal for OpenDrive {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        if range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::new(5, 1))); // 0.5
        }
        let up_excursion = (bar.high - bar.open).abs();
        let dn_excursion = (bar.low - bar.open).abs();
        let max_excursion = up_excursion.max(dn_excursion);
        let drive = max_excursion / range;
        Ok(SignalValue::Scalar(drive))
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
    fn test_od_always_ready() {
        let od = OpenDrive::new("od").unwrap();
        assert!(od.is_ready());
    }

    #[test]
    fn test_od_no_range() {
        let mut od = OpenDrive::new("od").unwrap();
        let result = od.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_od_open_at_low_all_upside() {
        // open=90, high=110, low=90 → up_excursion=20, dn_excursion=0, range=20 → drive=1.0
        let mut od = OpenDrive::new("od").unwrap();
        let result = od.update_bar(&bar("90", "110", "90", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_od_open_at_high_all_downside() {
        // open=110, high=110, low=90 → up_excursion=0, dn_excursion=20, range=20 → drive=1.0
        let mut od = OpenDrive::new("od").unwrap();
        let result = od.update_bar(&bar("110", "110", "90", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_od_open_at_midpoint() {
        // open=100, high=110, low=90 → up=10, dn=10, range=20 → drive=0.5
        let mut od = OpenDrive::new("od").unwrap();
        let result = od.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0.5)));
    }
}
