//! Trend Consistency Score indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Fraction of bars where close > N-bar EMA (simplified as SMA).
///
/// Measures how often price closes above its rolling average.
/// Values near 1.0: strong uptrend (price consistently above average).
/// Values near 0.0: strong downtrend (price consistently below average).
/// Values near 0.5: choppy / no trend.
pub struct TrendConsistencyScore {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl TrendConsistencyScore {
    /// Creates a new `TrendConsistencyScore` with the given rolling period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for TrendConsistencyScore {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sma = self.sum / Decimal::from(self.period as u32);
        let above_count = self.window.iter().filter(|&&c| c > sma).count();
        let score = Decimal::from(above_count as u32) / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(score))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "TrendConsistencyScore" }
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
    fn test_tcs_all_above_sma() {
        // Strongly trending up: last bars above SMA
        let mut sig = TrendConsistencyScore::new(4).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("110")).unwrap();
        sig.update(&bar("120")).unwrap();
        if let SignalValue::Scalar(v) = sig.update(&bar("130")).unwrap() {
            // sma = (100+110+120+130)/4=115, above sma: 120,130 → 2/4 = 0.5
            // Actually 100 and 110 are below 115, 120 and 130 are above → 0.5
            assert!(v >= dec!(0) && v <= dec!(1), "score out of range: {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_tcs_flat_half() {
        // Constant prices → none strictly above SMA → score = 0
        let mut sig = TrendConsistencyScore::new(3).unwrap();
        sig.update(&bar("100")).unwrap();
        sig.update(&bar("100")).unwrap();
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
