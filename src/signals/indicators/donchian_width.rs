//! Donchian Channel Width indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Width of the Donchian channel: `rolling_high - rolling_low`.
///
/// Measures the price range over the rolling period.
/// Wide channels: high volatility / trending market.
/// Narrow channels: low volatility / consolidation / breakout setup.
pub struct DonchianWidth {
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl DonchianWidth {
    /// Creates a new `DonchianWidth` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for DonchianWidth {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let rolling_high = self.highs.iter().cloned().fold(Decimal::MIN, Decimal::max);
        let rolling_low = self.lows.iter().cloned().fold(Decimal::MAX, Decimal::min);
        Ok(SignalValue::Scalar(rolling_high - rolling_low))
    }

    fn is_ready(&self) -> bool { self.highs.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.highs.clear(); self.lows.clear(); }
    fn name(&self) -> &str { "DonchianWidth" }
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
    fn test_dw_basic_width() {
        // high=120, low=80 over 2 bars → width=40
        let mut sig = DonchianWidth::new(2).unwrap();
        sig.update(&bar("120", "90")).unwrap();
        let v = sig.update(&bar("110", "80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(40)));
    }

    #[test]
    fn test_dw_single_bar() {
        // Period 1 → width = bar's own range
        let mut sig = DonchianWidth::new(1).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }
}
