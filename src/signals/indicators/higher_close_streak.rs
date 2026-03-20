//! Higher Close Streak indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Higher Close Streak — counts consecutive bars where `close > prev_close`.
///
/// Resets to 0 as soon as close does not exceed the previous close.
///
/// Returns [`SignalValue::Unavailable`] on the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HigherCloseStreak;
/// use fin_primitives::signals::Signal;
///
/// let hcs = HigherCloseStreak::new("hcs").unwrap();
/// assert_eq!(hcs.period(), 1);
/// ```
pub struct HigherCloseStreak {
    name: String,
    prev_close: Option<Decimal>,
    streak: u32,
}

impl HigherCloseStreak {
    /// # Errors
    /// Never errors — provided for API consistency.
    pub fn new(name: impl Into<String>) -> Result<Self, FinError> {
        Ok(Self { name: name.into(), prev_close: None, streak: 0 })
    }

    /// Returns the current streak count.
    pub fn streak(&self) -> u32 { self.streak }
}

impl Signal for HigherCloseStreak {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                if bar.close > pc {
                    self.streak += 1;
                } else {
                    self.streak = 0;
                }
                SignalValue::Scalar(Decimal::from(self.streak))
            }
        };
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
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
    fn test_hcs_unavailable_first_bar() {
        let mut hcs = HigherCloseStreak::new("hcs").unwrap();
        assert_eq!(hcs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_hcs_ascending_streak() {
        let mut hcs = HigherCloseStreak::new("hcs").unwrap();
        hcs.update_bar(&bar("100")).unwrap();
        for i in 1u32..=4 {
            let v = hcs.update_bar(&bar(&(100 + i).to_string())).unwrap();
            assert_eq!(v, SignalValue::Scalar(Decimal::from(i)));
        }
    }

    #[test]
    fn test_hcs_streak_resets_on_flat() {
        let mut hcs = HigherCloseStreak::new("hcs").unwrap();
        hcs.update_bar(&bar("100")).unwrap();
        hcs.update_bar(&bar("101")).unwrap();
        hcs.update_bar(&bar("102")).unwrap();
        // flat bar — streak resets
        let result = hcs.update_bar(&bar("102")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hcs_reset() {
        let mut hcs = HigherCloseStreak::new("hcs").unwrap();
        hcs.update_bar(&bar("100")).unwrap();
        hcs.update_bar(&bar("101")).unwrap();
        hcs.reset();
        assert_eq!(hcs.streak(), 0);
        assert_eq!(hcs.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
