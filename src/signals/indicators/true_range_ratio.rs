//! True Range Ratio — current True Range divided by ATR(period).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// True Range Ratio — how the current bar's true range compares to its average.
///
/// Defined as `TR(current) / ATR(period)`:
///
/// - **> 1**: the current bar has an above-average range (expanded volatility).
/// - **= 1**: the current bar's range equals the average.
/// - **< 1**: the current bar has a below-average range (compressed volatility).
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen,
/// or when ATR is zero.
///
/// Unlike [`crate::signals::indicators::Natr`] (which normalises ATR by the close price),
/// this indicator normalises the *current* true range by the *average* true range,
/// making it a self-referencing volatility expansion/compression metric.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrueRangeRatio;
/// use fin_primitives::signals::Signal;
/// let trr = TrueRangeRatio::new("trr_14", 14).unwrap();
/// assert_eq!(trr.period(), 14);
/// ```
pub struct TrueRangeRatio {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    tr_values: VecDeque<Decimal>,
    last_tr: Decimal,
}

impl TrueRangeRatio {
    /// Constructs a new `TrueRangeRatio`.
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
            last_tr: Decimal::ZERO,
        })
    }
}

impl Signal for TrueRangeRatio {
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
        let tr = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(pc) => {
                let hl = bar.high - bar.low;
                let hc = (bar.high - pc).abs();
                let lc = (bar.low - pc).abs();
                hl.max(hc).max(lc)
            }
        };
        self.prev_close = Some(bar.close);
        self.last_tr = tr;

        self.tr_values.push_back(tr);
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

        if atr.is_zero() {
            return Ok(SignalValue::Unavailable);
        }

        let ratio = tr
            .checked_div(atr)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.tr_values.clear();
        self.last_tr = Decimal::ZERO;
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
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(c.parse().unwrap()).unwrap(),
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
    fn test_trr_invalid_period() {
        assert!(TrueRangeRatio::new("trr", 0).is_err());
    }

    #[test]
    fn test_trr_unavailable_before_period() {
        let mut trr = TrueRangeRatio::new("trr", 3).unwrap();
        for i in 0..3u32 {
            let v = trr.update_bar(&bar("105", "95", &(100 + i).to_string())).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
        assert!(!trr.is_ready());
    }

    #[test]
    fn test_trr_constant_range_equals_one() {
        let mut trr = TrueRangeRatio::new("trr", 3).unwrap();
        // All bars have range = 10, so ATR = 10, current TR = 10, ratio = 1.
        for i in 0..4u32 {
            trr.update_bar(&bar("105", "95", &(100 + i).to_string())).unwrap();
        }
        let v = trr.update_bar(&bar("105", "95", "103")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_trr_expanded_range_greater_than_one() {
        let mut trr = TrueRangeRatio::new("trr", 3).unwrap();
        for i in 0..4u32 {
            trr.update_bar(&bar("101", "99", &(100 + i).to_string())).unwrap(); // range = 2
        }
        // Spike bar: range = 50
        let v = trr.update_bar(&bar("130", "80", "100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r > dec!(1), "expected > 1 for expanded range, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_trr_compressed_range_less_than_one() {
        let mut trr = TrueRangeRatio::new("trr", 3).unwrap();
        for i in 0..4u32 {
            trr.update_bar(&bar("120", "80", &(100 + i).to_string())).unwrap(); // range = 40
        }
        // Tiny bar: range = 1
        let v = trr.update_bar(&bar("100.5", "99.5", "100")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert!(r < dec!(1), "expected < 1 for compressed range, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_trr_reset() {
        let mut trr = TrueRangeRatio::new("trr", 3).unwrap();
        for i in 0..5u32 {
            trr.update_bar(&bar("105", "95", &(100 + i).to_string())).unwrap();
        }
        assert!(trr.is_ready());
        trr.reset();
        assert!(!trr.is_ready());
    }

    #[test]
    fn test_trr_period_and_name() {
        let trr = TrueRangeRatio::new("my_trr", 14).unwrap();
        assert_eq!(trr.period(), 14);
        assert_eq!(trr.name(), "my_trr");
    }
}
