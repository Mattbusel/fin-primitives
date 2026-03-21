//! Pivot Range Width indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Pivot Range Width.
///
/// Computes the width of the classic pivot point support/resistance range,
/// normalized as a percentage of the pivot point itself.
///
/// Classic pivot point: `P = (H + L + C) / 3`
/// Support 1: `S1 = 2*P - H`
/// Resistance 1: `R1 = 2*P - L`
/// Range width: `R1 - S1 = 2*P - L - (2*P - H) = H - L`
///
/// So the pivot range R1−S1 equals the bar's range. The interesting measure is
/// how this range compares to the pivot itself:
///
/// Formula: `prw = (R1 - S1) / P = (high - low) / pivot_point`
///
/// Rolling: `mean(prw, period)`
///
/// Higher values indicate wider pivot ranges relative to price level (volatile instrument).
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PivotRangeWidth;
/// use fin_primitives::signals::Signal;
/// let prw = PivotRangeWidth::new("prw_14", 14).unwrap();
/// assert_eq!(prw.period(), 14);
/// ```
pub struct PivotRangeWidth {
    name: String,
    period: usize,
    widths: VecDeque<Decimal>,
}

impl PivotRangeWidth {
    /// Constructs a new `PivotRangeWidth`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, widths: VecDeque::with_capacity(period) })
    }
}

impl Signal for PivotRangeWidth {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let pivot = (bar.high + bar.low + bar.close)
            .checked_div(Decimal::from(3u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let width = if pivot.is_zero() {
            Decimal::ZERO
        } else {
            let range = bar.high - bar.low;
            range.checked_div(pivot).ok_or(FinError::ArithmeticOverflow)?
        };

        self.widths.push_back(width);
        if self.widths.len() > self.period {
            self.widths.pop_front();
        }
        if self.widths.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.widths.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.widths.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.widths.clear();
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
        assert!(matches!(PivotRangeWidth::new("prw", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut prw = PivotRangeWidth::new("prw", 3).unwrap();
        assert_eq!(prw.update_bar(&bar("120", "100", "110")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_zero_range_gives_zero() {
        // h=l=c=100 → range=0, width=0
        let mut prw = PivotRangeWidth::new("prw", 3).unwrap();
        for _ in 0..3 {
            prw.update_bar(&bar("100", "100", "100")).unwrap();
        }
        let v = prw.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_positive_width() {
        let mut prw = PivotRangeWidth::new("prw", 3).unwrap();
        for _ in 0..3 {
            prw.update_bar(&bar("120", "100", "110")).unwrap();
        }
        let v = prw.update_bar(&bar("120", "100", "110")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut prw = PivotRangeWidth::new("prw", 2).unwrap();
        prw.update_bar(&bar("120", "100", "110")).unwrap();
        prw.update_bar(&bar("120", "100", "110")).unwrap();
        assert!(prw.is_ready());
        prw.reset();
        assert!(!prw.is_ready());
    }
}
