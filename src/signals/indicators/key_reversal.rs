//! Key Reversal — detects key reversal bar patterns.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Key Reversal — detects bullish (`+1`) and bearish (`-1`) key reversal bars.
///
/// A **bullish key reversal** occurs when:
/// 1. Current bar's low is below the prior bar's low (new low made).
/// 2. Current close is above the prior bar's close (reversal up).
///
/// A **bearish key reversal** occurs when:
/// 1. Current bar's high is above the prior bar's high (new high made).
/// 2. Current close is below the prior bar's close (reversal down).
///
/// Returns:
/// - `+1`: bullish key reversal.
/// - `-1`: bearish key reversal.
/// - `0`: no key reversal pattern.
/// - [`SignalValue::Unavailable`] for the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::KeyReversal;
/// use fin_primitives::signals::Signal;
/// let kr = KeyReversal::new("kr");
/// assert_eq!(kr.period(), 1);
/// ```
pub struct KeyReversal {
    name: String,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    prev_close: Option<Decimal>,
}

impl KeyReversal {
    /// Constructs a new `KeyReversal`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prev_high: None,
            prev_low: None,
            prev_close: None,
        }
    }
}

impl Signal for KeyReversal {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match (self.prev_high, self.prev_low, self.prev_close) {
            (Some(ph), Some(pl), Some(pc)) => {
                let bullish = bar.low < pl && bar.close > pc;
                let bearish = bar.high > ph && bar.close < pc;
                if bullish {
                    SignalValue::Scalar(Decimal::ONE)
                } else if bearish {
                    SignalValue::Scalar(Decimal::NEGATIVE_ONE)
                } else {
                    SignalValue::Scalar(Decimal::ZERO)
                }
            }
            _ => SignalValue::Unavailable,
        };
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let op = lp;
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_kr_first_bar_unavailable() {
        let mut s = KeyReversal::new("kr");
        assert!(!s.is_ready());
        assert_eq!(s.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
        assert!(s.is_ready());
    }

    #[test]
    fn test_kr_bullish_reversal() {
        let mut s = KeyReversal::new("kr");
        // Prior: H=110, L=90, C=95 (bearish day)
        s.update_bar(&bar("110","90","95")).unwrap();
        // Current: L < 90 (new low), C > 95 (reversal up) → bullish
        let v = s.update_bar(&bar("108","85","100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_kr_bearish_reversal() {
        let mut s = KeyReversal::new("kr");
        // Prior: H=110, L=90, C=105 (bullish day)
        s.update_bar(&bar("110","90","105")).unwrap();
        // Current: H > 110 (new high), C < 105 (reversal down) → bearish
        let v = s.update_bar(&bar("115","92","100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_kr_no_reversal_gives_zero() {
        let mut s = KeyReversal::new("kr");
        s.update_bar(&bar("110","90","100")).unwrap();
        // Normal bar, no reversal pattern
        let v = s.update_bar(&bar("108","92","102")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_kr_reset() {
        let mut s = KeyReversal::new("kr");
        s.update_bar(&bar("110","90","100")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
        assert_eq!(s.update_bar(&bar("110","90","100")).unwrap(), SignalValue::Unavailable);
    }
}
