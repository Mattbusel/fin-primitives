//! Positive Volume Index — cumulative price change index on up-volume days.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Positive Volume Index (PVI) — cumulative price change tracker for up-volume sessions.
///
/// Starts at 1000.0 and updates only when current volume is **greater than** the previous
/// bar's volume (indicating crowd/noise activity with higher participation):
/// - Updated bar: `PVI = prev_PVI * (1 + (close - prev_close) / prev_close)`
/// - Unchanged bar: `PVI = prev_PVI`
///
/// A rising PVI signals the crowd is pushing prices up on heavy volume.
/// A falling PVI indicates crowd-driven selling. Typically paired with NVI.
///
/// Returns [`SignalValue::Unavailable`] for the first bar (needs a previous bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PositiveVolumeIndex;
/// use fin_primitives::signals::Signal;
/// let pvi = PositiveVolumeIndex::new("pvi");
/// assert_eq!(pvi.period(), 1);
/// ```
pub struct PositiveVolumeIndex {
    name: String,
    pvi: Decimal,
    prev_close: Option<Decimal>,
    prev_volume: Option<Decimal>,
}

impl PositiveVolumeIndex {
    /// Constructs a new `PositiveVolumeIndex` starting at 1000.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pvi: Decimal::from(1000u32),
            prev_close: None,
            prev_volume: None,
        }
    }
}

impl Signal for PositiveVolumeIndex {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match (self.prev_close, self.prev_volume) {
            (Some(pc), Some(pv)) => {
                // Only update PVI on up-volume bars
                if bar.volume > pv && !pc.is_zero() {
                    let ret = (bar.close - pc)
                        .checked_div(pc)
                        .ok_or(FinError::ArithmeticOverflow)?;
                    self.pvi = self.pvi * (Decimal::ONE + ret);
                }
                SignalValue::Scalar(self.pvi)
            }
            _ => SignalValue::Unavailable,
        };
        self.prev_close = Some(bar.close);
        self.prev_volume = Some(bar.volume);
        Ok(result)
    }

    fn reset(&mut self) {
        self.pvi = Decimal::from(1000u32);
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
    fn test_pvi_first_bar_unavailable() {
        let mut s = PositiveVolumeIndex::new("pvi");
        assert!(!s.is_ready());
        assert_eq!(s.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
        assert!(s.is_ready());
    }

    #[test]
    fn test_pvi_unchanged_on_low_volume() {
        let mut s = PositiveVolumeIndex::new("pvi");
        s.update_bar(&bar("100", "1000")).unwrap();
        // Lower volume → PVI unchanged
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110", "500")).unwrap() {
            assert_eq!(v, dec!(1000), "PVI should not change on down-volume: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pvi_rises_on_high_volume_up_day() {
        let mut s = PositiveVolumeIndex::new("pvi");
        s.update_bar(&bar("100", "1000")).unwrap();
        // Higher volume + price rise → PVI increases
        if let SignalValue::Scalar(v) = s.update_bar(&bar("110", "2000")).unwrap() {
            assert!(v > dec!(1000), "PVI should rise on high-volume up day: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pvi_falls_on_high_volume_down_day() {
        let mut s = PositiveVolumeIndex::new("pvi");
        s.update_bar(&bar("100", "1000")).unwrap();
        // Higher volume + price fall → PVI decreases
        if let SignalValue::Scalar(v) = s.update_bar(&bar("90", "2000")).unwrap() {
            assert!(v < dec!(1000), "PVI should fall on high-volume down day: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pvi_reset() {
        let mut s = PositiveVolumeIndex::new("pvi");
        s.update_bar(&bar("100", "1000")).unwrap();
        s.update_bar(&bar("110", "2000")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
        assert_eq!(s.update_bar(&bar("100", "1000")).unwrap(), SignalValue::Unavailable);
    }
}
