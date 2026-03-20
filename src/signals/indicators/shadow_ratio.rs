//! Shadow Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of total wick (shadow) length relative to body length.
///
/// `(upper_wick + lower_wick) / |close - open|`
///
/// High values: large wicks relative to body (indecision, reversal signals).
/// Low values: small wicks, price moves cleanly from open to close.
/// Bars with zero body (doji) contribute a fixed value of 1.0.
/// Bars with zero total wick contribute 0.0.
pub struct ShadowRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl ShadowRatio {
    /// Creates a new `ShadowRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for ShadowRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = (bar.close - bar.open).abs();
        let body_high = bar.open.max(bar.close);
        let body_low = bar.open.min(bar.close);
        let total_wick = (bar.high - body_high) + (body_low - bar.low);

        let ratio = if body.is_zero() {
            Decimal::ONE
        } else {
            total_wick / body
        };

        self.window.push_back(ratio);
        self.sum += ratio;
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
    fn name(&self) -> &str { "ShadowRatio" }
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
    fn test_sr_no_wicks() {
        // open=low, close=high → body=range, no wicks → ratio = 0
        let mut sig = ShadowRatio::new(2).unwrap();
        sig.update(&bar("90", "110", "90", "110")).unwrap(); // no wicks, ratio=0
        let v = sig.update(&bar("90", "110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_sr_equal_body_and_wicks() {
        // body=10, upper_wick=5, lower_wick=5 → total_wick=10 → ratio=1
        let mut sig = ShadowRatio::new(2).unwrap();
        // open=95, close=105 (body=10), high=110, low=90
        sig.update(&bar("95", "110", "90", "105")).unwrap();
        let v = sig.update(&bar("95", "110", "90", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }
}
