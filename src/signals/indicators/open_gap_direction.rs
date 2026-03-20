//! Open Gap Direction — sign of the opening gap relative to the prior close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Open Gap Direction — direction of the current bar's opening gap vs the prior close.
///
/// Emits:
/// - **+1**: gap up (`open > prev_close`).
/// - **−1**: gap down (`open < prev_close`).
/// - **0**: no gap (`open == prev_close`).
///
/// Returns [`SignalValue::Unavailable`] until 2 bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenGapDirection;
/// use fin_primitives::signals::Signal;
/// let ogd = OpenGapDirection::new("ogd");
/// assert_eq!(ogd.period(), 1);
/// ```
pub struct OpenGapDirection {
    name: String,
    prev_close: Option<Decimal>,
}

impl OpenGapDirection {
    /// Constructs a new `OpenGapDirection`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_close: None }
    }
}

impl Signal for OpenGapDirection {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let Some(pc) = self.prev_close {
            let signal = if bar.open > pc {
                Decimal::ONE
            } else if bar.open < pc {
                Decimal::NEGATIVE_ONE
            } else {
                Decimal::ZERO
            };
            Ok(SignalValue::Scalar(signal))
        } else {
            Ok(SignalValue::Unavailable)
        };

        self.prev_close = Some(bar.close);
        result
    }

    fn reset(&mut self) {
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: cp.max(op), low: cp.min(op), close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ogd_first_bar_unavailable() {
        let mut s = OpenGapDirection::new("ogd");
        assert_eq!(s.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
        assert!(s.is_ready());
    }

    #[test]
    fn test_ogd_gap_up() {
        let mut s = OpenGapDirection::new("ogd");
        s.update_bar(&bar("100", "102")).unwrap(); // prev_close=102
        let v = s.update_bar(&bar("105", "107")).unwrap(); // open=105 > 102 → +1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ogd_gap_down() {
        let mut s = OpenGapDirection::new("ogd");
        s.update_bar(&bar("100", "102")).unwrap(); // prev_close=102
        let v = s.update_bar(&bar("100", "98")).unwrap(); // open=100 < 102 → -1
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_ogd_no_gap() {
        let mut s = OpenGapDirection::new("ogd");
        s.update_bar(&bar("100", "102")).unwrap(); // prev_close=102
        let v = s.update_bar(&bar("102", "104")).unwrap(); // open=102 == prev_close → 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ogd_reset() {
        let mut s = OpenGapDirection::new("ogd");
        s.update_bar(&bar("100", "102")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
