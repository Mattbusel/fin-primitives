//! Price Position Rank indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Percentile rank of current close within the rolling window.
///
/// Returns a value from 0 to 1:
/// - 0.0 = current close is the lowest in the window
/// - 1.0 = current close is the highest in the window
/// Useful for identifying overbought/oversold conditions over N bars.
pub struct PricePositionRank {
    period: usize,
    closes: VecDeque<Decimal>,
}

impl PricePositionRank {
    /// Creates a new `PricePositionRank` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, closes: VecDeque::with_capacity(period) })
    }
}

impl Signal for PricePositionRank {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.closes.push_back(bar.close);
        if self.closes.len() > self.period {
            self.closes.pop_front();
        }
        if self.closes.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let current = bar.close;
        let below = self.closes.iter().filter(|&&c| c < current).count();
        let total = self.closes.len() - 1; // exclude current bar itself
        if total == 0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let rank = Decimal::from(below as u32) / Decimal::from(total as u32);
        Ok(SignalValue::Scalar(rank))
    }

    fn is_ready(&self) -> bool { self.closes.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.closes.clear(); }
    fn name(&self) -> &str { "PricePositionRank" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_ppr_at_top() {
        // Final bar is highest → rank = 1
        let mut sig = PricePositionRank::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("101")).unwrap();
        let v = sig.update(&bar("102")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ppr_at_bottom() {
        // Final bar is lowest → rank = 0
        let mut sig = PricePositionRank::new(3).unwrap();
        sig.update(&bar("102")).unwrap();
        sig.update(&bar("101")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ppr_at_middle() {
        // Final bar is middle → rank = 0.5
        let mut sig = PricePositionRank::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("102")).unwrap();
        let v = sig.update(&bar("101")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }
}
