//! Normalised Average True Range.

use crate::error::FinError;
use crate::signals::indicators::Atr;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Normalised ATR: ATR expressed as a percentage of the closing price (TA-Lib's `NATR`).
///
/// ```text
/// NATR = ATR(period) / close * 100
/// ```
///
/// The ATR is Wilder's ([`Atr::new`]). Before 4.0 it was a simple moving average of the
/// true range, which never converges to TA-Lib's `NATR`.
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (the first bar cannot produce a true range because there is no previous close), and
/// when `close` is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Natr;
/// use fin_primitives::signals::Signal;
///
/// let natr = Natr::new("natr14", 14).unwrap();
/// assert_eq!(natr.period(), 14);
/// ```
pub struct Natr {
    name: String,
    atr: Atr,
}

impl Natr {
    /// Constructs a new `Natr`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        let name = name.into();
        let atr = Atr::new(format!("{name}_atr"), period)?;
        Ok(Self { name, atr })
    }
}

impl Signal for Natr {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.atr.period()
    }

    fn is_ready(&self) -> bool {
        self.atr.is_ready()
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let SignalValue::Scalar(atr) = self.atr.update(bar)? else {
            return Ok(SignalValue::Unavailable);
        };
        if bar.close.is_zero() {
            return Ok(SignalValue::Unavailable);
        }
        let natr = atr
            .checked_div(bar.close)
            .ok_or(FinError::ArithmeticOverflow)?
            * Decimal::from(100u32);
        Ok(SignalValue::Scalar(natr))
    }

    fn reset(&mut self) {
        self.atr.reset();
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
    fn test_natr_invalid_period() {
        assert!(Natr::new("n", 0).is_err());
    }

    #[test]
    fn test_natr_unavailable_on_first_bar() {
        let mut natr = Natr::new("n", 1).unwrap();
        assert_eq!(
            natr.update_bar(&bar("100", "110", "90", "100")).unwrap(),
            SignalValue::Unavailable
        );
    }

    #[test]
    fn test_natr_period1_after_two_bars() {
        let mut natr = Natr::new("n", 1).unwrap();
        natr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        // TR = max(120-80, |120-100|, |80-100|) = 40; close = 110
        let v = natr.update_bar(&bar("100", "120", "80", "110")).unwrap();
        // NATR = 40/110 * 100
        if let SignalValue::Scalar(val) = v {
            let expected = dec!(40) / dec!(110) * dec!(100);
            assert_eq!(val, expected);
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_natr_is_ready() {
        let mut natr = Natr::new("n", 2).unwrap();
        assert!(!natr.is_ready());
        natr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        natr.update_bar(&bar("100", "101", "99", "100")).unwrap();
        assert!(!natr.is_ready()); // 1 TR so far, need 2
        natr.update_bar(&bar("100", "102", "98", "100")).unwrap();
        assert!(natr.is_ready());
    }

    #[test]
    fn test_natr_uses_wilder_atr() {
        let mut natr = Natr::new("n", 2).unwrap();
        natr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        natr.update_bar(&bar("100", "104", "100", "100")).unwrap(); // TR 4
        natr.update_bar(&bar("100", "102", "100", "100")).unwrap(); // TR 2: seed 3
                                                                    // TR 6: Wilder (3 * 1 + 6) / 2 = 4.5 (a simple average would say 4).
        let v = natr.update_bar(&bar("100", "106", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(4.5)));
    }

    #[test]
    fn test_natr_reset() {
        let mut natr = Natr::new("n", 1).unwrap();
        natr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        natr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert!(natr.is_ready());
        natr.reset();
        assert!(!natr.is_ready());
    }
}
