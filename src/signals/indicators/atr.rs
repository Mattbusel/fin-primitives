//! Average True Range (ATR) indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Average True Range over `period` bars.
///
/// True Range for each bar is: `max(high - low, |high - prev_close|, |low - prev_close|)`.
/// ATR is the simple moving average of true range over `period` bars.
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars have been seen (the first
/// bar cannot produce a true range because there is no previous close).
///
/// ATR is commonly used for volatility measurement, position sizing, and stop-loss placement.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::Atr;
/// use fin_primitives::signals::Signal;
/// let atr = Atr::new("atr_14", 14).unwrap();
/// assert_eq!(atr.period(), 14);
/// ```
pub struct Atr {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    tr_values: VecDeque<Decimal>,
}

impl Atr {
    /// Constructs a new `Atr` with the given name and period.
    ///
    /// # Errors
    /// Returns [`crate::error::FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, crate::error::FinError> {
        if period == 0 {
            return Err(crate::error::FinError::InvalidPeriod(period));
        }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            tr_values: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for Atr {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let true_range = match self.prev_close {
            None => {
                // First bar: no previous close, use high - low only.
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(prev) => {
                let hl = bar.range();
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
        Ok(SignalValue::Scalar(atr))
    }

    fn is_ready(&self) -> bool {
        self.tr_values.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
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
    fn test_atr_unavailable_on_first_bar() {
        let mut atr = Atr::new("atr1", 1).unwrap();
        let v = atr.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
        assert!(!atr.is_ready());
    }

    #[test]
    fn test_atr_period_1_ready_after_two_bars() {
        let mut atr = Atr::new("atr1", 1).unwrap();
        atr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        let v = atr.update_bar(&bar("100", "115", "95", "110")).unwrap();
        // TR = max(115-95, |115-100|, |95-100|) = max(20, 15, 5) = 20
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
        assert!(atr.is_ready());
    }

    #[test]
    fn test_atr_uses_prev_close_for_gap() {
        // Simulate a gap: prev close = 100, new bar opens and trades much higher
        let mut atr = Atr::new("atr1", 1).unwrap();
        atr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        // Gap up: bar trades 120-115, but prev close = 100
        // TR = max(120-115, |120-100|, |115-100|) = max(5, 20, 15) = 20
        let v = atr.update_bar(&bar("118", "120", "115", "119")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_atr_period_0_fails() {
        assert!(Atr::new("atr0", 0).is_err());
    }

    #[test]
    fn test_atr_period_3_averages_true_ranges() {
        let mut atr = Atr::new("atr3", 3).unwrap();
        // Bar 0: establishes prev_close = 100, no TR
        atr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        // Bar 1: TR = max(120-80, |120-100|, |80-100|) = max(40, 20, 20) = 40
        atr.update_bar(&bar("100", "120", "80", "110")).unwrap();
        // Bar 2: TR = max(115-105, |115-110|, |105-110|) = max(10, 5, 5) = 10
        atr.update_bar(&bar("112", "115", "105", "108")).unwrap();
        // Bar 3: TR = max(112-100, |112-108|, |100-108|) = max(12, 4, 8) = 12
        let v = atr.update_bar(&bar("110", "112", "100", "105")).unwrap();
        // ATR(3) = (40 + 10 + 12) / 3 = 20.666...
        if let SignalValue::Scalar(atr_val) = v {
            let expected = dec!(62) / dec!(3);
            assert_eq!(atr_val, expected);
        } else {
            panic!("expected Scalar after period bars");
        }
    }

    #[test]
    fn test_atr_unavailable_before_period_filled() {
        let mut atr = Atr::new("atr3", 3).unwrap();
        atr.update_bar(&bar("100", "110", "90", "100")).unwrap(); // bar 0, no TR
        let v1 = atr.update_bar(&bar("100", "120", "80", "110")).unwrap(); // TR=40, 1 of 3
        let v2 = atr.update_bar(&bar("112", "115", "105", "108")).unwrap(); // TR=10, 2 of 3
        assert_eq!(v1, SignalValue::Unavailable);
        assert_eq!(v2, SignalValue::Unavailable);
        assert!(!atr.is_ready());
    }
}
