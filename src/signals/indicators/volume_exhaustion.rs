//! Volume Exhaustion indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume Exhaustion — counts consecutive bars where volume is declining while
/// price continues moving in its current direction.
///
/// Outputs:
/// - `+n` → `n` consecutive bullish bars (`close > open`) with decreasing volume  
/// - `-n` → `n` consecutive bearish bars (`close < open`) with decreasing volume  
/// - `0`  → volume did not decrease, direction changed, or doji  
///
/// A high `|n|` value suggests the move may be losing momentum (volume exhaustion).
///
/// Returns [`SignalValue::Unavailable`] on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeExhaustion;
/// use fin_primitives::signals::Signal;
///
/// let ve = VolumeExhaustion::new("ve").unwrap();
/// assert_eq!(ve.period(), 1);
/// ```
pub struct VolumeExhaustion {
    name: String,
    prev_volume: Option<Decimal>,
    streak: i64,
}

impl VolumeExhaustion {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_volume: None, streak: 0 })
    }

    /// Returns the current exhaustion streak.
    pub fn streak(&self) -> i64 { self.streak }
}

impl Signal for VolumeExhaustion {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_volume.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_volume {
            None => SignalValue::Unavailable,
            Some(pv) => {
                let vol_declining = bar.volume < pv;
                let bullish = bar.close > bar.open;
                let bearish = bar.close < bar.open;

                self.streak = if vol_declining && bullish {
                    if self.streak >= 0 { self.streak + 1 } else { 1 }
                } else if vol_declining && bearish {
                    if self.streak <= 0 { self.streak - 1 } else { -1 }
                } else {
                    0
                };

                SignalValue::Scalar(Decimal::from(self.streak))
            }
        };
        self.prev_volume = Some(bar.volume);
        Ok(result)
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

    fn bar(o: &str, c: &str, v: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let vq = Quantity::new(v.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp, low: op, close: cp,
            volume: vq,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ve_unavailable_first_bar() {
        let mut ve = VolumeExhaustion::new("ve").unwrap();
        assert_eq!(ve.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ve_bullish_with_declining_volume() {
        let mut ve = VolumeExhaustion::new("ve").unwrap();
        ve.update_bar(&bar("100", "105", "1000")).unwrap(); // seed
        // bullish, volume declining: 900 < 1000
        let r1 = ve.update_bar(&bar("105", "110", "900")).unwrap();
        let r2 = ve.update_bar(&bar("110", "115", "800")).unwrap();
        assert_eq!(r1, SignalValue::Scalar(dec!(1)));
        assert_eq!(r2, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_ve_increasing_volume_resets() {
        let mut ve = VolumeExhaustion::new("ve").unwrap();
        ve.update_bar(&bar("100", "105", "1000")).unwrap();
        ve.update_bar(&bar("105", "110", "900")).unwrap(); // streak=1
        // volume increases: reset
        let result = ve.update_bar(&bar("110", "115", "1200")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ve_reset() {
        let mut ve = VolumeExhaustion::new("ve").unwrap();
        ve.update_bar(&bar("100", "105", "1000")).unwrap();
        ve.update_bar(&bar("105", "110", "900")).unwrap();
        assert_eq!(ve.streak(), 1);
        ve.reset();
        assert_eq!(ve.streak(), 0);
        assert_eq!(ve.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
    }
}
