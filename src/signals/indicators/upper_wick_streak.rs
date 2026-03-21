//! Upper Wick Streak indicator.
//!
//! Counts consecutive bars where the upper wick exceeds a minimum fraction of
//! the bar's range, detecting sustained rejection from above.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Upper Wick Streak — count of consecutive bars where the upper wick makes up
/// at least `min_pct` of the bar's range.
///
/// For each bar:
/// ```text
/// upper_wick  = high - max(open, close)
/// range       = high - low
/// fraction    = upper_wick / range          (0 when range == 0)
/// ```
///
/// The streak increments when `fraction >= min_pct`, and resets to zero
/// otherwise (including flat bars).
///
/// - **High streak**: market is persistently rejecting higher prices — a
///   reliable supply zone or resistance level.
/// - **0**: current bar does not exhibit significant upper rejection.
///
/// Returns [`SignalValue::Unavailable`] on the first bar.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] is never triggered; `new` always succeeds.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpperWickStreak;
/// use fin_primitives::signals::Signal;
/// use rust_decimal_macros::dec;
///
/// let uws = UpperWickStreak::new("uws", dec!(0.3)).unwrap();
/// assert_eq!(uws.period(), 1);
/// ```
pub struct UpperWickStreak {
    name: String,
    min_pct: Decimal,
    streak: u32,
    prev_seen: bool,
}

impl UpperWickStreak {
    /// Constructs a new `UpperWickStreak`.
    ///
    /// `min_pct` is the minimum fraction of the bar's range that the upper
    /// wick must occupy to count as a rejection bar (e.g. `dec!(0.3)` for 30%).
    ///
    /// # Errors
    /// Never fails; returns `Ok` always.
    pub fn new(name: impl Into<String>, min_pct: Decimal) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), min_pct, streak: 0, prev_seen: false })
    }
}

impl Signal for UpperWickStreak {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_seen }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.prev_seen = true;
        let range = bar.range();
        let fraction = if range.is_zero() {
            Decimal::ZERO
        } else {
            let upper_wick = bar.high - bar.open.max(bar.close);
            upper_wick
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        if fraction >= self.min_pct {
            self.streak += 1;
        } else {
            self.streak = 0;
        }

        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.streak = 0;
        self.prev_seen = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(o.parse().unwrap()).unwrap(),
            high: Price::new(h.parse().unwrap()).unwrap(),
            low: Price::new(l.parse().unwrap()).unwrap(),
            close: Price::new(c.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_uws_no_upper_wick_zero() {
        // Close at high: no upper wick → fraction = 0 → streak = 0
        let mut uws = UpperWickStreak::new("uws", dec!(0.3)).unwrap();
        let v = uws.update_bar(&bar("100", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_uws_large_upper_wick_increments() {
        let mut uws = UpperWickStreak::new("uws", dec!(0.3)).unwrap();
        // open=100, high=110, low=90, close=100: upper_wick=10, range=20, frac=0.5 >= 0.3
        let v = uws.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_uws_streak_increments() {
        let mut uws = UpperWickStreak::new("uws", dec!(0.3)).unwrap();
        for _ in 0..3 {
            uws.update_bar(&bar("100", "110", "90", "100")).unwrap(); // 0.5 >= 0.3
        }
        let v = uws.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(4)));
    }

    #[test]
    fn test_uws_streak_resets_on_small_wick() {
        let mut uws = UpperWickStreak::new("uws", dec!(0.3)).unwrap();
        uws.update_bar(&bar("100", "110", "90", "100")).unwrap(); // streak=1
        uws.update_bar(&bar("100", "110", "90", "100")).unwrap(); // streak=2
        // Close near high: upper_wick=(110-108)=2, range=20, frac=0.1 < 0.3 → reset
        let v = uws.update_bar(&bar("100", "110", "90", "108")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_uws_ready_after_first_bar() {
        let mut uws = UpperWickStreak::new("uws", dec!(0.3)).unwrap();
        assert!(!uws.is_ready());
        uws.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert!(uws.is_ready());
    }

    #[test]
    fn test_uws_period_is_one() {
        let uws = UpperWickStreak::new("uws", dec!(0.3)).unwrap();
        assert_eq!(uws.period(), 1);
    }

    #[test]
    fn test_uws_reset() {
        let mut uws = UpperWickStreak::new("uws", dec!(0.3)).unwrap();
        uws.update_bar(&bar("100", "110", "90", "100")).unwrap();
        uws.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert!(uws.is_ready());
        uws.reset();
        assert!(!uws.is_ready());
    }
}
