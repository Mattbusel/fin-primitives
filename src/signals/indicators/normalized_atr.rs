//! Normalized ATR indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Normalized ATR (Average True Range normalized by close price).
///
/// Divides the standard ATR by the current closing price to produce a
/// percentage-based volatility measure that is comparable across different
/// price levels and instruments.
///
/// Formula:
/// - `tr = max(high, prev_close) - min(low, prev_close)`
/// - `atr = mean(tr, period)`
/// - `natr = atr / close * 100`
///
/// Returns a percentage value — e.g., 2.0 means ATR is 2% of close.
///
/// Returns `SignalValue::Unavailable` until `period + 1` bars accumulated.
/// Returns `SignalValue::Scalar(0.0)` when close is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::NormalizedAtr;
/// use fin_primitives::signals::Signal;
/// let natr = NormalizedAtr::new("natr_14", 14).unwrap();
/// assert_eq!(natr.period(), 14);
/// ```
pub struct NormalizedAtr {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    true_ranges: VecDeque<Decimal>,
    last_close: Decimal,
}

impl NormalizedAtr {
    /// Constructs a new `NormalizedAtr`.
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
            true_ranges: VecDeque::with_capacity(period),
            last_close: Decimal::ZERO,
        })
    }
}

impl Signal for NormalizedAtr {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.last_close = bar.close;

        let tr = if let Some(prev_c) = self.prev_close {
            let high_ext = bar.high.max(prev_c);
            let low_ext = bar.low.min(prev_c);
            high_ext - low_ext
        } else {
            self.prev_close = Some(bar.close);
            return Ok(SignalValue::Unavailable);
        };

        self.prev_close = Some(bar.close);
        self.true_ranges.push_back(tr);
        if self.true_ranges.len() > self.period {
            self.true_ranges.pop_front();
        }
        if self.true_ranges.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if bar.close.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let sum: Decimal = self.true_ranges.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let atr = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        let natr = atr
            .checked_div(bar.close)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(natr))
    }

    fn is_ready(&self) -> bool {
        self.true_ranges.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.true_ranges.clear();
        self.last_close = Decimal::ZERO;
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
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(NormalizedAtr::new("natr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut natr = NormalizedAtr::new("natr", 3).unwrap();
        assert_eq!(natr.update_bar(&bar("12", "10", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_natr_positive() {
        let mut natr = NormalizedAtr::new("natr", 3).unwrap();
        for _ in 0..4 {
            natr.update_bar(&bar("110", "90", "100")).unwrap();
        }
        let v = natr.update_bar(&bar("110", "90", "100")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut natr = NormalizedAtr::new("natr", 2).unwrap();
        for _ in 0..3 {
            natr.update_bar(&bar("12", "10", "11")).unwrap();
        }
        assert!(natr.is_ready());
        natr.reset();
        assert!(!natr.is_ready());
    }
}
