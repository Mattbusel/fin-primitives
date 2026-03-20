//! Price Compression Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Ratio of N-bar price range to sum of individual bar ranges.
///
/// `(rolling_high - rolling_low) / Σ(bar ranges)`
///
/// Values near 0: bars cancel each other (choppy, directionless).
/// Values near 1: bars stack in same direction (trending, no overlap).
/// Measures directional efficiency of recent price movement.
pub struct PriceCompressionRatio {
    period: usize,
    window: VecDeque<BarInput>,
    range_sum: Decimal,
}

impl PriceCompressionRatio {
    /// Creates a new `PriceCompressionRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            window: VecDeque::with_capacity(period),
            range_sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceCompressionRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.range_sum += range;
        self.window.push_back(*bar);
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.range_sum -= old.high - old.low;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        if self.range_sum.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let period_high = self.window.iter().map(|b| b.high).fold(Decimal::MIN, Decimal::max);
        let period_low = self.window.iter().map(|b| b.low).fold(Decimal::MAX, Decimal::min);
        let net_range = period_high - period_low;
        Ok(SignalValue::Scalar(net_range / self.range_sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.range_sum = Decimal::ZERO; }
    fn name(&self) -> &str { "PriceCompressionRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_pcr_non_overlapping() {
        // bar1: high=110, low=100 (range=10); bar2: high=120, low=110 (range=10)
        // net_range = 120-100 = 20, range_sum = 20 → ratio = 1.0
        let mut sig = PriceCompressionRatio::new(2).unwrap();
        sig.update(&bar("110", "100")).unwrap();
        let v = sig.update(&bar("120", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_pcr_full_overlap() {
        // Identical bars: high=110, low=90 (range=20 each)
        // net_range = 110-90 = 20, range_sum = 40 → ratio = 0.5
        let mut sig = PriceCompressionRatio::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }
}
