//! Momentum Streak indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Momentum Streak — counts consecutive closes above or below the prior close.
///
/// ```text
/// streak_t = streak_{t-1} + 1   if close_t > close_{t-1}
///           streak_{t-1} - 1   if close_t < close_{t-1}
///           0                   if close_t == close_{t-1}
/// ```
///
/// A rising streak (positive) indicates persistent buying; a falling streak
/// (negative) indicates persistent selling. Extreme streaks often precede reversals.
///
/// Returns [`SignalValue::Unavailable`] until the second bar is seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MomentumStreak;
/// use fin_primitives::signals::Signal;
///
/// let ms = MomentumStreak::new("ms").unwrap();
/// assert_eq!(ms.period(), 1);
/// ```
pub struct MomentumStreak {
    name: String,
    prev_close: Option<Decimal>,
    streak: Decimal,
}

impl MomentumStreak {
    /// Creates a new `MomentumStreak`.
    ///
    /// # Errors
    /// Always succeeds; returns `Ok(Self)`.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self {
            name: name.into(),
            prev_close: None,
            streak: Decimal::ZERO,
        })
    }

    /// Returns the current streak value.
    pub fn streak(&self) -> Decimal { self.streak }
}

impl Signal for MomentumStreak {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;

        let prev = match self.prev_close {
            None => {
                self.prev_close = Some(close);
                return Ok(SignalValue::Unavailable);
            }
            Some(p) => p,
        };
        self.prev_close = Some(close);

        if close > prev {
            self.streak = if self.streak < Decimal::ZERO { Decimal::ONE } else { self.streak + Decimal::ONE };
        } else if close < prev {
            self.streak = if self.streak > Decimal::ZERO { -Decimal::ONE } else { self.streak - Decimal::ONE };
        } else {
            self.streak = Decimal::ZERO;
        }

        Ok(SignalValue::Scalar(self.streak))
    }

    fn is_ready(&self) -> bool {
        self.prev_close.is_some()
    }

    fn period(&self) -> usize {
        1
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.streak = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_streak_first_bar_unavailable() {
        let mut ms = MomentumStreak::new("ms").unwrap();
        assert_eq!(ms.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_streak_up_bars_count() {
        let mut ms = MomentumStreak::new("ms").unwrap();
        ms.update_bar(&bar("100")).unwrap();
        assert_eq!(ms.update_bar(&bar("101")).unwrap(), SignalValue::Scalar(dec!(1)));
        assert_eq!(ms.update_bar(&bar("102")).unwrap(), SignalValue::Scalar(dec!(2)));
        assert_eq!(ms.update_bar(&bar("103")).unwrap(), SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_streak_resets_on_direction_change() {
        let mut ms = MomentumStreak::new("ms").unwrap();
        ms.update_bar(&bar("100")).unwrap();
        ms.update_bar(&bar("101")).unwrap(); // streak=1
        ms.update_bar(&bar("102")).unwrap(); // streak=2
        // Now down bar — streak resets to -1
        assert_eq!(ms.update_bar(&bar("101")).unwrap(), SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_streak_flat_resets_to_zero() {
        let mut ms = MomentumStreak::new("ms").unwrap();
        ms.update_bar(&bar("100")).unwrap();
        ms.update_bar(&bar("101")).unwrap(); // streak=1
        assert_eq!(ms.update_bar(&bar("101")).unwrap(), SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_streak_reset() {
        let mut ms = MomentumStreak::new("ms").unwrap();
        ms.update_bar(&bar("100")).unwrap();
        ms.update_bar(&bar("101")).unwrap();
        assert!(ms.is_ready());
        ms.reset();
        assert!(!ms.is_ready());
        assert_eq!(ms.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
