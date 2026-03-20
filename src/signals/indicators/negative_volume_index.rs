//! Negative Volume Index — cumulative price change index on down-volume days.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Negative Volume Index (NVI) — cumulative price change tracker for down-volume sessions.
///
/// Starts at 1000.0 and updates only when current volume is **less than** the previous
/// bar's volume (indicating "smart money" activity with lower participation):
/// - Updated bar: `NVI = prev_NVI * (1 + (close - prev_close) / prev_close)`
/// - Unchanged bar: `NVI = prev_NVI`
///
/// A rising NVI suggests informed traders are accumulating (rising price on low volume).
/// A falling NVI suggests distribution. Typically used against its own MA for signals.
///
/// Returns [`SignalValue::Unavailable`] for the first bar (needs a previous bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NegativeVolumeIndex;
/// use fin_primitives::signals::Signal;
/// let nvi = NegativeVolumeIndex::new("nvi");
/// assert_eq!(nvi.period(), 1);
/// ```
pub struct NegativeVolumeIndex {
    name: String,
    nvi: Decimal,
    prev_close: Option<Decimal>,
    prev_volume: Option<Decimal>,
}

impl NegativeVolumeIndex {
    /// Constructs a new `NegativeVolumeIndex` starting at 1000.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            nvi: Decimal::from(1000u32),
            prev_close: None,
            prev_volume: None,
        }
    }
}

impl Signal for NegativeVolumeIndex {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match (self.prev_close, self.prev_volume) {
            (Some(pc), Some(pv)) => {
                // Only update NVI on down-volume bars
                if bar.volume < pv && !pc.is_zero() {
                    let ret = (bar.close - pc)
                        .checked_div(pc)
                        .ok_or(FinError::ArithmeticOverflow)?;
                    self.nvi = self.nvi * (Decimal::ONE + ret);
                }
                SignalValue::Scalar(self.nvi)
            }
            _ => SignalValue::Unavailable,
        };
        self.prev_close = Some(bar.close);
        self.prev_volume = Some(bar.volume);
        Ok(result)
    }

    fn reset(&mut self) {
        self.nvi = Decimal::from(1000u32);
        self.prev_close = None;
        self.prev_volume = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str, vol: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_nvi_first_bar_unavailable() {
        let mut s = NegativeVolumeIndex::new("nvi");
        assert!(!s.is_ready());
        assert_eq!(s.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_nvi_unchanged_on_high_volume() {
        let mut s = NegativeVolumeIndex::new("nvi");
        s.update_bar(&bar("100", "1000")).unwrap();
        // Higher volume → NVI unchanged
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110", "2000")).unwrap() {
            assert_eq!(v, dec!(1000), "NVI should not change on up-volume: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_nvi_rises_on_low_volume_up_day() {
        let mut s = NegativeVolumeIndex::new("nvi");
        s.update_bar(&bar("100", "1000")).unwrap();
        // Lower volume + price rise → NVI increases
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110", "500")).unwrap() {
            assert!(v > dec!(1000), "NVI should rise on low-volume up day: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_nvi_falls_on_low_volume_down_day() {
        let mut s = NegativeVolumeIndex::new("nvi");
        s.update_bar(&bar("100", "1000")).unwrap();
        // Lower volume + price fall → NVI decreases
        if let SignalValue::Scalar(v) = s.update_bar(&bar("90", "500")).unwrap() {
            assert!(v < dec!(1000), "NVI should fall on low-volume down day: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_nvi_reset() {
        let mut s = NegativeVolumeIndex::new("nvi");
        s.update_bar(&bar("100", "1000")).unwrap();
        s.update_bar(&bar("110", "500")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
        assert_eq!(s.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }
}
