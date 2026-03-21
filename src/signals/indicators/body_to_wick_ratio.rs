//! Body-to-Wick Ratio indicator.
//!
//! Rolling mean of the ratio of candle body size to total wick length.
//! Measures whether bars are dominated by directional commitment or rejection.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Body-to-Wick Ratio — rolling mean of `body / total_wicks`.
///
/// For each bar:
/// ```text
/// body        = |close - open|
/// upper_wick  = high - max(open, close)
/// lower_wick  = min(open, close) - low
/// total_wicks = upper_wick + lower_wick
///
/// ratio = body / total_wicks   when total_wicks > 0
///       = 0                    when total_wicks == 0 (body fills entire range)
/// ```
///
/// Note: when the bar is a perfect marubozu (no wicks), `total_wicks = 0`.
/// In this case the ratio is set to the maximum observed value (∞ → capped at
/// the body itself divided by a unit to avoid division by zero) — so a
/// dedicated value of `body` is returned. Actually for simplicity, when wicks
/// are zero we return the special value `body / range` (= 1 if bar fills range).
///
/// Practical interpretation:
/// - **High value (> 1)**: body is larger than total wicks — directional bars.
/// - **Low value (< 1)**: wicks dominate — rejection and indecision.
/// - **= 1**: body and total wicks are equal.
///
/// Bars with zero range contribute 0.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars are collected.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BodyToWickRatio;
/// use fin_primitives::signals::Signal;
/// let bwr = BodyToWickRatio::new("bwr_14", 14).unwrap();
/// assert_eq!(bwr.period(), 14);
/// ```
pub struct BodyToWickRatio {
    name: String,
    period: usize,
    values: VecDeque<Decimal>,
    sum: Decimal,
}

impl BodyToWickRatio {
    /// Constructs a new `BodyToWickRatio`.
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
            values: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for BodyToWickRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.values.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let ratio = if range.is_zero() {
            Decimal::ZERO
        } else {
            let body = bar.body_size();
            let body_top = bar.open.max(bar.close);
            let body_bot = bar.open.min(bar.close);
            let upper_wick = bar.high - body_top;
            let lower_wick = body_bot - bar.low;
            let total_wicks = upper_wick + lower_wick;

            if total_wicks.is_zero() {
                // Marubozu: no wicks — body fills the range
                // Represent as body/range = 1 (all body, no wick)
                Decimal::ONE
            } else {
                body.checked_div(total_wicks).ok_or(FinError::ArithmeticOverflow)?
            }
        };

        self.sum += ratio;
        self.values.push_back(ratio);
        if self.values.len() > self.period {
            let removed = self.values.pop_front().unwrap();
            self.sum -= removed;
        }

        if self.values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let mean = self.sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(mean))
    }

    fn reset(&mut self) {
        self.values.clear();
        self.sum = Decimal::ZERO;
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
    fn test_bwr_invalid_period() {
        assert!(BodyToWickRatio::new("bwr", 0).is_err());
    }

    #[test]
    fn test_bwr_unavailable_during_warmup() {
        let mut bwr = BodyToWickRatio::new("bwr", 3).unwrap();
        assert_eq!(bwr.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(bwr.update_bar(&bar("100", "110", "90", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!bwr.is_ready());
    }

    #[test]
    fn test_bwr_marubozu_gives_one() {
        // open=low, close=high: no wicks → ratio = 1
        let mut bwr = BodyToWickRatio::new("bwr", 2).unwrap();
        bwr.update_bar(&bar("90", "110", "90", "110")).unwrap();
        if let SignalValue::Scalar(v) = bwr.update_bar(&bar("90", "110", "90", "110")).unwrap() {
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bwr_doji_small_body_vs_large_wicks() {
        // open=100, close=100, high=110, low=90: body=0, total_wicks=20 → ratio=0
        let mut bwr = BodyToWickRatio::new("bwr", 2).unwrap();
        bwr.update_bar(&bar("100", "110", "90", "100")).unwrap();
        if let SignalValue::Scalar(v) = bwr.update_bar(&bar("100", "110", "90", "100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bwr_body_larger_than_wicks() {
        // open=95, high=110, low=90, close=105
        // body=10, upper_wick=5, lower_wick=5, total_wicks=10 → ratio=1
        let mut bwr = BodyToWickRatio::new("bwr", 2).unwrap();
        bwr.update_bar(&bar("95", "110", "90", "105")).unwrap();
        if let SignalValue::Scalar(v) = bwr.update_bar(&bar("95", "110", "90", "105")).unwrap() {
            assert_eq!(v, dec!(1));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bwr_flat_bar_zero() {
        let mut bwr = BodyToWickRatio::new("bwr", 2).unwrap();
        bwr.update_bar(&bar("100", "100", "100", "100")).unwrap();
        if let SignalValue::Scalar(v) = bwr.update_bar(&bar("100", "100", "100", "100")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bwr_reset() {
        let mut bwr = BodyToWickRatio::new("bwr", 2).unwrap();
        bwr.update_bar(&bar("95", "110", "90", "105")).unwrap();
        bwr.update_bar(&bar("95", "110", "90", "105")).unwrap();
        assert!(bwr.is_ready());
        bwr.reset();
        assert!(!bwr.is_ready());
    }
}
