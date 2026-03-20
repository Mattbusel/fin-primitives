//! Volatility Break indicator — flags single-bar moves larger than N × ATR.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volatility Break — detects when a single bar's move is unusually large.
///
/// On each bar, computes the current bar's range as `(high - low)` and compares it to
/// `multiplier × ATR(period)`. Returns:
/// - `1` when the bar range exceeds `multiplier × ATR` (a volatility breakout).
/// - `0` otherwise.
///
/// This is useful for detecting gap opens, news events, or sudden liquidity vacuums.
/// Uses the simple ATR (SMA of true range) over `period` bars.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolatilityBreak;
/// use fin_primitives::signals::Signal;
/// let vb = VolatilityBreak::new("vb_14", 14, rust_decimal_macros::dec!(2)).unwrap();
/// assert_eq!(vb.period(), 14);
/// ```
pub struct VolatilityBreak {
    name: String,
    period: usize,
    multiplier: Decimal,
    prev_close: Option<Decimal>,
    tr_values: VecDeque<Decimal>,
}

impl VolatilityBreak {
    /// Constructs a new `VolatilityBreak`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    /// Returns [`FinError::InvalidInput`] if `multiplier` is not positive.
    pub fn new(
        name: impl Into<String>,
        period: usize,
        multiplier: Decimal,
    ) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        if multiplier <= Decimal::ZERO {
            return Err(FinError::InvalidInput("multiplier must be positive".to_owned()));
        }
        Ok(Self {
            name: name.into(),
            period,
            multiplier,
            prev_close: None,
            tr_values: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolatilityBreak {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.tr_values.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tr = match self.prev_close {
            None => {
                self.prev_close = Some(bar.close);
                return Ok(SignalValue::Unavailable);
            }
            Some(pc) => {
                let hl = bar.range();
                let hc = (bar.high - pc).abs();
                let lc = (bar.low - pc).abs();
                hl.max(hc).max(lc)
            }
        };
        self.prev_close = Some(bar.close);

        self.tr_values.push_back(tr);
        if self.tr_values.len() > self.period {
            self.tr_values.pop_front();
        }

        if self.tr_values.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.tr_values.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let atr = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let threshold = atr
            .checked_mul(self.multiplier)
            .ok_or(FinError::ArithmeticOverflow)?;

        let current_range = bar.range();
        let breakout = if current_range > threshold { Decimal::ONE } else { Decimal::ZERO };
        Ok(SignalValue::Scalar(breakout))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.tr_values.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn normal_bar(c: &str) -> OhlcvBar {
        let p: Decimal = c.parse().unwrap();
        let hp = Price::new(p + dec!(1)).unwrap();
        let lp = Price::new(p - dec!(1)).unwrap();
        let cp = Price::new(p).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vb_invalid_period() {
        assert!(VolatilityBreak::new("vb", 0, dec!(2)).is_err());
    }

    #[test]
    fn test_vb_invalid_multiplier() {
        assert!(VolatilityBreak::new("vb", 5, dec!(0)).is_err());
        assert!(VolatilityBreak::new("vb", 5, dec!(-1)).is_err());
    }

    #[test]
    fn test_vb_unavailable_before_period() {
        let mut vb = VolatilityBreak::new("vb", 3, dec!(2)).unwrap();
        for i in 0..3u32 {
            let v = vb.update_bar(&normal_bar(&(100 + i).to_string())).unwrap();
            assert_eq!(v, SignalValue::Unavailable);
        }
        assert!(!vb.is_ready());
    }

    #[test]
    fn test_vb_normal_bar_returns_zero() {
        let mut vb = VolatilityBreak::new("vb", 3, dec!(2)).unwrap();
        for i in 0..4u32 {
            vb.update_bar(&normal_bar(&(100 + i).to_string())).unwrap();
        }
        let v = vb.update_bar(&normal_bar("104")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vb_large_bar_returns_one() {
        let mut vb = VolatilityBreak::new("vb", 3, dec!(2)).unwrap();
        // Build up normal ATR ≈ 2 with normal bars (range=2).
        for i in 0..4u32 {
            vb.update_bar(&normal_bar(&(100 + i).to_string())).unwrap();
        }
        // Now a bar with range = 50 (much > 2 * ATR ≈ 4).
        let v = vb.update_bar(&bar("150", "100", "125")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_vb_reset() {
        let mut vb = VolatilityBreak::new("vb", 3, dec!(2)).unwrap();
        for i in 0..5u32 {
            vb.update_bar(&normal_bar(&(100 + i).to_string())).unwrap();
        }
        assert!(vb.is_ready());
        vb.reset();
        assert!(!vb.is_ready());
    }

    #[test]
    fn test_vb_period_and_name() {
        let vb = VolatilityBreak::new("my_vb", 14, dec!(2)).unwrap();
        assert_eq!(vb.period(), 14);
        assert_eq!(vb.name(), "my_vb");
    }
}
