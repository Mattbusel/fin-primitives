//! Up-Close Streak indicator.
//!
//! Counts the current unbroken streak of consecutive bars where the close
//! exceeded the previous bar's close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Up-Close Streak: count of consecutive higher closes.
///
/// Returns the number of bars in the current unbroken run where
/// `close[i] > close[i-1]`. Resets to zero on any bar where the close
/// is less than or equal to the prior close.
///
/// - **High value**: sustained upward momentum.
/// - **0**: streak just broke (current bar closed <= prior close).
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior close).
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] is never triggered; `new` always succeeds.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpCloseStreak;
/// use fin_primitives::signals::Signal;
///
/// let ucs = UpCloseStreak::new("ucs").unwrap();
/// assert_eq!(ucs.period(), 1);
/// assert!(!ucs.is_ready());
/// ```
pub struct UpCloseStreak {
    name: String,
    prev_close: Option<Decimal>,
    streak: u32,
    seen_bars: usize,
}

impl UpCloseStreak {
    /// Constructs a new `UpCloseStreak`.
    ///
    /// # Errors
    /// Never fails; returns `Ok` always.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_close: None, streak: 0, seen_bars: 0 })
    }
}

impl Signal for UpCloseStreak {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        1
    }

    fn is_ready(&self) -> bool {
        self.seen_bars >= 2
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let close = bar.close;
        self.seen_bars += 1;

        let Some(prev) = self.prev_close else {
            self.prev_close = Some(close);
            return Ok(SignalValue::Unavailable);
        };

        if close > prev {
            self.streak += 1;
        } else {
            self.streak = 0;
        }

        self.prev_close = Some(close);
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.streak = 0;
        self.seen_bars = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(close: &str) -> OhlcvBar {
        let p = Price::new(close.parse().unwrap()).unwrap();
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
    fn test_ucs_first_bar_unavailable() {
        let mut ucs = UpCloseStreak::new("ucs").unwrap();
        assert_eq!(ucs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!ucs.is_ready());
    }

    #[test]
    fn test_ucs_ready_after_second_bar() {
        let mut ucs = UpCloseStreak::new("ucs").unwrap();
        ucs.update_bar(&bar("100")).unwrap();
        ucs.update_bar(&bar("101")).unwrap();
        assert!(ucs.is_ready());
    }

    #[test]
    fn test_ucs_counts_rising_closes() {
        let mut ucs = UpCloseStreak::new("ucs").unwrap();
        ucs.update_bar(&bar("100")).unwrap();
        assert_eq!(ucs.update_bar(&bar("101")).unwrap(), SignalValue::Scalar(dec!(1)));
        assert_eq!(ucs.update_bar(&bar("102")).unwrap(), SignalValue::Scalar(dec!(2)));
        assert_eq!(ucs.update_bar(&bar("103")).unwrap(), SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_ucs_resets_on_down_close() {
        let mut ucs = UpCloseStreak::new("ucs").unwrap();
        ucs.update_bar(&bar("100")).unwrap();
        ucs.update_bar(&bar("101")).unwrap();
        ucs.update_bar(&bar("102")).unwrap();
        // Down bar: streak resets
        let v = ucs.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ucs_resets_on_equal_close() {
        let mut ucs = UpCloseStreak::new("ucs").unwrap();
        ucs.update_bar(&bar("100")).unwrap();
        ucs.update_bar(&bar("101")).unwrap();
        // Equal close breaks the streak
        let v = ucs.update_bar(&bar("101")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ucs_resumes_after_reset() {
        let mut ucs = UpCloseStreak::new("ucs").unwrap();
        ucs.update_bar(&bar("100")).unwrap();
        ucs.update_bar(&bar("101")).unwrap();
        ucs.update_bar(&bar("100")).unwrap(); // break
        let v = ucs.update_bar(&bar("102")).unwrap(); // new streak starts
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ucs_period_is_one() {
        let ucs = UpCloseStreak::new("ucs").unwrap();
        assert_eq!(ucs.period(), 1);
    }

    #[test]
    fn test_ucs_reset() {
        let mut ucs = UpCloseStreak::new("ucs").unwrap();
        ucs.update_bar(&bar("100")).unwrap();
        ucs.update_bar(&bar("101")).unwrap();
        assert!(ucs.is_ready());
        ucs.reset();
        assert!(!ucs.is_ready());
        assert_eq!(ucs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
