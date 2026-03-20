//! Open-High-Low-Close Average indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `(open + high + low + close) / 4`.
///
/// The OHLC average (also called the four-price doji average) captures the full
/// bar information equally. Smoother than close-only SMA, less biased than
/// typical price (which weights close twice).
pub struct OpenHighLowCloseAvg {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl OpenHighLowCloseAvg {
    /// Creates a new `OpenHighLowCloseAvg` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for OpenHighLowCloseAvg {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let ohlc4 = (bar.open + bar.high + bar.low + bar.close) / Decimal::from(4u32);
        self.window.push_back(ohlc4);
        self.sum += ohlc4;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "OpenHighLowCloseAvg" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(o: &str, h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: o.parse().unwrap(),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_ohlca_symmetric_bar() {
        // open=close=100, high=110, low=90 → ohlc4 = (100+110+90+100)/4 = 100
        let mut sig = OpenHighLowCloseAvg::new(2).unwrap();
        sig.update(&bar("100", "110", "90", "100")).unwrap();
        let v = sig.update(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_ohlca_all_same() {
        // All values = 100 → ohlc4 = 100, avg = 100
        let mut sig = OpenHighLowCloseAvg::new(3).unwrap();
        sig.update(&bar("100", "100", "100", "100")).unwrap();
        sig.update(&bar("100", "100", "100", "100")).unwrap();
        let v = sig.update(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }
}
