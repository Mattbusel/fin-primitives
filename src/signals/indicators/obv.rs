//! On-Balance Volume (OBV) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// On-Balance Volume (OBV) — cumulative volume flow indicator.
///
/// OBV adds the bar's volume when close > previous close, subtracts it when
/// close < previous close, and makes no change on an unchanged close.
/// The running cumulative value reflects buying/selling pressure.
///
/// Ready after the second bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Obv;
/// use fin_primitives::signals::Signal;
/// use fin_primitives::signals::BarInput;
/// use rust_decimal_macros::dec;
///
/// let mut obv = Obv::new("obv");
/// obv.update(&BarInput::new(dec!(100), dec!(105), dec!(98), dec!(99), dec!(1000))).unwrap();
/// obv.update(&BarInput::new(dec!(102), dec!(107), dec!(100), dec!(100), dec!(1500))).unwrap();
/// // close went up 100→102, so OBV += 1500 → 1500
/// ```
pub struct Obv {
    name: String,
    obv: Decimal,
    prev_close: Option<Decimal>,
}

impl Obv {
    /// Constructs a new `Obv` indicator.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            obv: Decimal::ZERO,
            prev_close: None,
        }
    }

    /// Returns the current OBV value.
    pub fn value(&self) -> Decimal {
        self.obv
    }
}

impl Signal for Obv {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(prev) = self.prev_close {
            if bar.close > prev {
                self.obv += bar.volume;
            } else if bar.close < prev {
                self.obv -= bar.volume;
            }
            // unchanged close: no OBV change
        }
        self.prev_close = Some(bar.close);
        if self.prev_close.is_some() && !self.obv.is_zero() || self.prev_close.is_some() {
            // Ready after first bar (prev_close is set)
        }
        match self.prev_close {
            None => Ok(SignalValue::Unavailable),
            Some(_) => Ok(SignalValue::Scalar(self.obv)),
        }
    }

    fn is_ready(&self) -> bool {
        self.prev_close.is_some()
    }

    fn period(&self) -> usize {
        2
    }

    fn reset(&mut self) {
        self.obv = Decimal::ZERO;
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Signal;
    use rust_decimal_macros::dec;

    fn bar(close: &str, vol: &str) -> BarInput {
        let c: Decimal = close.parse().unwrap();
        let v: Decimal = vol.parse().unwrap();
        BarInput::new(c, c, c, c, v)
    }

    #[test]
    fn test_obv_first_bar_returns_zero() {
        let mut obv = Obv::new("obv");
        let result = obv.update(&bar("100", "1000")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_obv_up_close_adds_volume() {
        let mut obv = Obv::new("obv");
        obv.update(&bar("100", "1000")).unwrap();
        let result = obv.update(&bar("102", "1500")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(1500)));
    }

    #[test]
    fn test_obv_down_close_subtracts_volume() {
        let mut obv = Obv::new("obv");
        obv.update(&bar("100", "1000")).unwrap();
        let result = obv.update(&bar("98", "800")).unwrap();
        assert_eq!(result, SignalValue::Scalar(dec!(-800)));
    }

    #[test]
    fn test_obv_unchanged_close_no_change() {
        let mut obv = Obv::new("obv");
        obv.update(&bar("100", "1000")).unwrap();
        obv.update(&bar("102", "500")).unwrap(); // OBV = 500
        let result = obv.update(&bar("102", "200")).unwrap(); // unchanged close
        assert_eq!(result, SignalValue::Scalar(dec!(500)));
    }

    #[test]
    fn test_obv_reset_clears_state() {
        let mut obv = Obv::new("obv");
        obv.update(&bar("100", "1000")).unwrap();
        obv.update(&bar("105", "2000")).unwrap();
        obv.reset();
        assert!(!obv.is_ready());
        assert_eq!(obv.value(), dec!(0));
    }
}
