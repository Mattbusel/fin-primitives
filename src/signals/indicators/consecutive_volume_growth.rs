//! Consecutive Volume Growth indicator -- streak of increasing-volume bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Consecutive Volume Growth -- the number of consecutive bars where volume
/// exceeded the previous bar's volume.
///
/// Resets to 0 when a bar's volume does not exceed the prior bar's volume.
/// Useful for detecting unusual sustained volume surges.
///
/// ```text
/// streak[t] = streak[t-1] + 1  if volume[t] > volume[t-1]
///           = 0                 otherwise
/// ```
///
/// Returns 0 on the first bar (no prior bar to compare).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ConsecutiveVolumeGrowth;
/// use fin_primitives::signals::Signal;
/// let cvg = ConsecutiveVolumeGrowth::new("cvg");
/// assert_eq!(cvg.period(), 1);
/// ```
pub struct ConsecutiveVolumeGrowth {
    name: String,
    prev_volume: Option<Decimal>,
    streak: u32,
}

impl ConsecutiveVolumeGrowth {
    /// Constructs a new `ConsecutiveVolumeGrowth`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_volume: None, streak: 0 }
    }
}

impl Signal for ConsecutiveVolumeGrowth {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.streak = match self.prev_volume {
            Some(pv) if bar.volume > pv => self.streak + 1,
            _ => 0,
        };
        self.prev_volume = Some(bar.volume);
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.prev_volume = None;
        self.streak = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(vol: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
        let v = Quantity::new(vol.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p, volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cvg_first_bar_is_zero() {
        let mut cvg = ConsecutiveVolumeGrowth::new("cvg");
        let v = cvg.update_bar(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cvg_growing_streak() {
        let mut cvg = ConsecutiveVolumeGrowth::new("cvg");
        cvg.update_bar(&bar("100")).unwrap();    // streak=0
        cvg.update_bar(&bar("200")).unwrap();    // streak=1
        cvg.update_bar(&bar("300")).unwrap();    // streak=2
        let v = cvg.update_bar(&bar("400")).unwrap(); // streak=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_cvg_resets_on_flat() {
        let mut cvg = ConsecutiveVolumeGrowth::new("cvg");
        cvg.update_bar(&bar("100")).unwrap(); // streak=0
        cvg.update_bar(&bar("200")).unwrap(); // streak=1
        cvg.update_bar(&bar("200")).unwrap(); // not growing -> streak=0
        let v = cvg.update_bar(&bar("300")).unwrap(); // streak=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cvg_resets_on_decrease() {
        let mut cvg = ConsecutiveVolumeGrowth::new("cvg");
        cvg.update_bar(&bar("500")).unwrap(); // streak=0
        cvg.update_bar(&bar("600")).unwrap(); // streak=1
        let v = cvg.update_bar(&bar("400")).unwrap(); // decrease -> streak=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cvg_reset() {
        let mut cvg = ConsecutiveVolumeGrowth::new("cvg");
        cvg.update_bar(&bar("100")).unwrap();
        cvg.update_bar(&bar("200")).unwrap();
        cvg.reset();
        let v = cvg.update_bar(&bar("300")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
