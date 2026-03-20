//! Wick-to-Body Ratio — rolling average of total wick length relative to body size.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Wick-to-Body Ratio — rolling average of `(upper_wick + lower_wick) / body_size`.
///
/// Measures how "tail-heavy" bars are on average over `period` bars:
/// - **High value (> 1)**: wicks dominate — price rejected from extremes (indecision/reversal).
/// - **Low value (near 0)**: body dominates — strong directional bars.
///
/// Only bars with non-zero body contribute to the average. Returns
/// [`SignalValue::Unavailable`] until `period` valid bars (non-zero body) have been collected,
/// or when no valid bars exist in the window.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::WickToBodyRatio;
/// use fin_primitives::signals::Signal;
/// let wbr = WickToBodyRatio::new("wbr_10", 10).unwrap();
/// assert_eq!(wbr.period(), 10);
/// ```
pub struct WickToBodyRatio {
    name: String,
    period: usize,
    window: VecDeque<Decimal>, // wick/body ratios for valid bars
    sum: Decimal,
}

impl WickToBodyRatio {
    /// Constructs a new `WickToBodyRatio`.
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
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for WickToBodyRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let body = if bar.close >= bar.open {
            bar.close - bar.open
        } else {
            bar.open - bar.close
        };

        if body.is_zero() {
            // Doji bars don't contribute
            return Ok(SignalValue::Unavailable);
        }

        let upper_wick = bar.high - bar.close.max(bar.open);
        let lower_wick = bar.open.min(bar.close) - bar.low;
        let total_wick = upper_wick + lower_wick;

        let ratio = total_wick
            .checked_div(body)
            .ok_or(FinError::ArithmeticOverflow)?;

        self.sum += ratio;
        self.window.push_back(ratio);
        if self.window.len() > self.period {
            let removed = self.window.pop_front().unwrap();
            self.sum -= removed;
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let avg = self.sum
            .checked_div(Decimal::from(self.window.len() as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(avg))
    }

    fn reset(&mut self) {
        self.window.clear();
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
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_wbr_invalid_period() {
        assert!(WickToBodyRatio::new("wbr", 0).is_err());
    }

    #[test]
    fn test_wbr_doji_skipped() {
        let mut s = WickToBodyRatio::new("wbr", 2).unwrap();
        // doji → skipped
        let v = s.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_wbr_no_wick_gives_zero() {
        let mut s = WickToBodyRatio::new("wbr", 2).unwrap();
        // open=90, close=110, high=110, low=90 → no wicks, body=20 → ratio=0
        s.update_bar(&bar("90", "110", "90", "110")).unwrap();
        let v = s.update_bar(&bar("90", "110", "90", "110")).unwrap();
        if let SignalValue::Scalar(r) = v {
            assert_eq!(r, dec!(0), "no-wick bar should give ratio 0: {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_wbr_non_negative() {
        let mut s = WickToBodyRatio::new("wbr", 3).unwrap();
        let bars = [
            bar("100", "115", "95", "110"),
            bar("110", "120", "105", "112"),
            bar("112", "118", "108", "115"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = s.update_bar(b).unwrap() {
                assert!(v >= dec!(0), "wick-body ratio must be non-negative: {v}");
            }
        }
    }

    #[test]
    fn test_wbr_reset() {
        let mut s = WickToBodyRatio::new("wbr", 2).unwrap();
        s.update_bar(&bar("90", "110", "90", "110")).unwrap();
        s.update_bar(&bar("90", "110", "90", "110")).unwrap();
        assert!(s.is_ready());
        s.reset();
        assert!(!s.is_ready());
    }
}
