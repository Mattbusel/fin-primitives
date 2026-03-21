//! Bar Polarity Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bar Polarity Ratio.
///
/// Tracks the rolling fraction of bars where `close > open` (bullish bars)
/// minus the fraction where `close < open` (bearish bars).
///
/// Formula: `polarity = (bull_count - bear_count) / period` ∈ [−1, +1]
///
/// Doji bars (close == open) count as neither.
///
/// - +1: all bars are bullish.
/// - −1: all bars are bearish.
/// - 0: equal mix of bull and bear bars.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarPolarityRatio;
/// use fin_primitives::signals::Signal;
/// let bpr = BarPolarityRatio::new("bpr_14", 14).unwrap();
/// assert_eq!(bpr.period(), 14);
/// ```
pub struct BarPolarityRatio {
    name: String,
    period: usize,
    /// +1 bull, -1 bear, 0 doji
    polarities: VecDeque<i8>,
}

impl BarPolarityRatio {
    /// Constructs a new `BarPolarityRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, polarities: VecDeque::with_capacity(period) })
    }
}

impl Signal for BarPolarityRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let p: i8 = if bar.close > bar.open {
            1
        } else if bar.close < bar.open {
            -1
        } else {
            0
        };
        self.polarities.push_back(p);
        if self.polarities.len() > self.period {
            self.polarities.pop_front();
        }
        if self.polarities.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: i32 = self.polarities.iter().map(|&x| i32::from(x)).sum();
        #[allow(clippy::cast_possible_truncation)]
        let ratio = Decimal::from(sum)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool {
        self.polarities.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.polarities.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let hi = op.max(cl);
        let lo = op.min(cl);
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(BarPolarityRatio::new("bpr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut bpr = BarPolarityRatio::new("bpr", 3).unwrap();
        assert_eq!(bpr.update_bar(&bar("10", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_all_bull_gives_one() {
        let mut bpr = BarPolarityRatio::new("bpr", 3).unwrap();
        for _ in 0..3 {
            bpr.update_bar(&bar("10", "11")).unwrap();
        }
        let v = bpr.update_bar(&bar("10", "11")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_all_bear_gives_minus_one() {
        let mut bpr = BarPolarityRatio::new("bpr", 3).unwrap();
        for _ in 0..3 {
            bpr.update_bar(&bar("11", "10")).unwrap();
        }
        let v = bpr.update_bar(&bar("11", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_reset() {
        let mut bpr = BarPolarityRatio::new("bpr", 2).unwrap();
        bpr.update_bar(&bar("10", "11")).unwrap();
        bpr.update_bar(&bar("10", "11")).unwrap();
        assert!(bpr.is_ready());
        bpr.reset();
        assert!(!bpr.is_ready());
    }
}
