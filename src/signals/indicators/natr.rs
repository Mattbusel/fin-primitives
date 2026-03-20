//! Normalised Average True Range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Normalised ATR — ATR expressed as a percentage of the closing price.
///
/// ```text
/// NATR = ATR(period) / close * 100
/// ```
///
/// The ATR is the simple moving average of true range over `period` bars.
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (the first bar cannot produce a true range because there is no previous close).
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
    period: usize,
    prev_close: Option<Decimal>,
    tr_values: VecDeque<Decimal>,
}

impl Natr {
    /// Constructs a new `Natr`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            tr_values: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Natr {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.tr_values.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let true_range = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(prev) => {
                let hl = bar.high - bar.low;
                let hc = (bar.high - prev).abs();
                let lc = (bar.low - prev).abs();
                hl.max(hc).max(lc)
            }
        };

        self.prev_close = Some(bar.close);
        self.tr_values.push_back(true_range);
        if self.tr_values.len() > self.period {
            self.tr_values.pop_front();
        }

        if self.tr_values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.tr_values.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let atr = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

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
        self.prev_close = None;
        self.tr_values.clear();
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
    fn test_natr_reset() {
        let mut natr = Natr::new("n", 1).unwrap();
        natr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        natr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert!(natr.is_ready());
        natr.reset();
        assert!(!natr.is_ready());
    }
}
