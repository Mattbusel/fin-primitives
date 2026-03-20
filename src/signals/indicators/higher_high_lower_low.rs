//! Higher-High / Lower-Low detector — identifies market structure patterns.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Higher-High / Lower-Low — detects trending bar structure relative to the prior bar.
///
/// Emits:
/// - **+1**: `high > prev_high` AND `low > prev_low` (higher-high, higher-low — bullish structure).
/// - **−1**: `high < prev_high` AND `low < prev_low` (lower-high, lower-low — bearish structure).
/// - **0**: mixed or equal (inside bar, outside bar, or flat comparison).
///
/// Returns [`SignalValue::Unavailable`] until 2 bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HigherHighLowerLow;
/// use fin_primitives::signals::Signal;
/// let hhll = HigherHighLowerLow::new("hhll");
/// assert_eq!(hhll.period(), 1);
/// ```
pub struct HigherHighLowerLow {
    name: String,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
}

impl HigherHighLowerLow {
    /// Constructs a new `HigherHighLowerLow`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_high: None, prev_low: None }
    }
}

impl Signal for HigherHighLowerLow {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_high.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            let signal = if bar.high > ph && bar.low > pl {
                Decimal::ONE       // bullish structure
            } else if bar.high < ph && bar.low < pl {
                Decimal::NEGATIVE_ONE  // bearish structure
            } else {
                Decimal::ZERO
            };
            Ok(SignalValue::Scalar(signal))
        } else {
            Ok(SignalValue::Unavailable)
        };

        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        result
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_hhll_first_bar_unavailable() {
        let mut s = HigherHighLowerLow::new("hhll");
        assert_eq!(s.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(s.is_ready());
    }

    #[test]
    fn test_hhll_bullish_structure() {
        let mut s = HigherHighLowerLow::new("hhll");
        s.update_bar(&bar("110", "90")).unwrap();
        let v = s.update_bar(&bar("115", "95")).unwrap(); // hh & hl → +1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_hhll_bearish_structure() {
        let mut s = HigherHighLowerLow::new("hhll");
        s.update_bar(&bar("110", "90")).unwrap();
        let v = s.update_bar(&bar("105", "85")).unwrap(); // lh & ll → -1
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_hhll_inside_bar_zero() {
        let mut s = HigherHighLowerLow::new("hhll");
        s.update_bar(&bar("120", "80")).unwrap(); // prior: [80,120]
        let v = s.update_bar(&bar("110", "90")).unwrap(); // inside → 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hhll_outside_bar_zero() {
        let mut s = HigherHighLowerLow::new("hhll");
        s.update_bar(&bar("110", "90")).unwrap(); // prior: [90,110]
        let v = s.update_bar(&bar("120", "80")).unwrap(); // outside → 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hhll_reset() {
        let mut s = HigherHighLowerLow::new("hhll");
        s.update_bar(&bar("110", "90")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
