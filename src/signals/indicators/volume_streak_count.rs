//! Volume Streak Count indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Streak count of consecutive bars with increasing (+) or decreasing (-) volume.
///
/// - Positive values: N consecutive bars of increasing volume
/// - Negative values: N consecutive bars of decreasing volume
/// - 0: volume unchanged from prior bar (resets streak)
/// - Always ready after the first bar.
pub struct VolumeStreakCount {
    prev_vol: Option<Decimal>,
    streak: i32,
}

impl VolumeStreakCount {
    /// Creates a new `VolumeStreakCount`.
    pub fn new() -> Self {
        Self { prev_vol: None, streak: 0 }
    }
}

impl Default for VolumeStreakCount {
    fn default() -> Self { Self::new() }
}

impl Signal for VolumeStreakCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pv) = self.prev_vol {
            if bar.volume > pv {
                self.streak = if self.streak > 0 { self.streak + 1 } else { 1 };
            } else if bar.volume < pv {
                self.streak = if self.streak < 0 { self.streak - 1 } else { -1 };
            } else {
                self.streak = 0;
            }
        }
        self.prev_vol = Some(bar.volume);
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn is_ready(&self) -> bool { true }
    fn period(&self) -> usize { 1 }
    fn reset(&mut self) { self.prev_vol = None; self.streak = 0; }
    fn name(&self) -> &str { "VolumeStreakCount" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(v: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(110),
            low: dec!(90),
            close: dec!(100),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vsc_increasing_streak() {
        let mut sig = VolumeStreakCount::new();
        sig.update(&bar("1000")).unwrap(); // first bar, streak=0
        sig.update(&bar("1100")).unwrap(); // +1 streak
        sig.update(&bar("1200")).unwrap(); // +2 streak
        let v = sig.update(&bar("1300")).unwrap(); // +3 streak
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_vsc_decreasing_streak() {
        let mut sig = VolumeStreakCount::new();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("900")).unwrap(); // -1
        let v = sig.update(&bar("800")).unwrap(); // -2
        assert_eq!(v, SignalValue::Scalar(dec!(-2)));
    }

    #[test]
    fn test_vsc_reset_on_equal() {
        let mut sig = VolumeStreakCount::new();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1100")).unwrap(); // +1
        let v = sig.update(&bar("1100")).unwrap(); // equal → 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
