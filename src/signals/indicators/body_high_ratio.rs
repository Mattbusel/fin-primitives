//! Body-to-High Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body-to-High Ratio.
///
/// Measures what fraction of the bar's full height from zero (the high price)
/// is made up by the body. This is different from body-to-range; it gives a sense
/// of the body size relative to absolute price level.
///
/// Per-bar formula: `bhr = |close - open| / high`
///
/// Rolling: `mean(bhr, period)`
///
/// - Higher values indicate bars with large bodies relative to their price level.
/// - Lower values indicate small bodies (doji-like or expensive/low-volatility bars).
/// - Returns 0 when high is zero.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyHighRatio;
/// use fin_primitives::signals::Signal;
/// let bhr = BodyHighRatio::new("bhr_14", 14).unwrap();
/// assert_eq!(bhr.period(), 14);
/// ```
pub struct BodyHighRatio {
    name: String,
    period: usize,
    ratios: VecDeque<Decimal>,
}

impl BodyHighRatio {
    /// Constructs a new `BodyHighRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, ratios: VecDeque::with_capacity(period) })
    }
}

impl Signal for BodyHighRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let bhr = if bar.high.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - bar.open)
                .abs()
                .checked_div(bar.high)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.ratios.push_back(bhr);
        if self.ratios.len() > self.period {
            self.ratios.pop_front();
        }
        if self.ratios.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.ratios.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.ratios.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.ratios.clear();
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
    fn test_period_zero_fails() {
        assert!(matches!(BodyHighRatio::new("bhr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut bhr = BodyHighRatio::new("bhr", 3).unwrap();
        assert_eq!(bhr.update_bar(&bar("10", "15", "8", "12")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_doji_gives_zero() {
        // open == close → body=0 → ratio=0
        let mut bhr = BodyHighRatio::new("bhr", 3).unwrap();
        for _ in 0..3 {
            bhr.update_bar(&bar("10", "15", "8", "10")).unwrap();
        }
        let v = bhr.update_bar(&bar("10", "15", "8", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_body_ratio_positive() {
        let mut bhr = BodyHighRatio::new("bhr", 3).unwrap();
        for _ in 0..3 {
            bhr.update_bar(&bar("10", "15", "8", "13")).unwrap();
        }
        let v = bhr.update_bar(&bar("10", "15", "8", "13")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut bhr = BodyHighRatio::new("bhr", 2).unwrap();
        bhr.update_bar(&bar("10", "15", "8", "12")).unwrap();
        bhr.update_bar(&bar("10", "15", "8", "12")).unwrap();
        assert!(bhr.is_ready());
        bhr.reset();
        assert!(!bhr.is_ready());
    }
}
