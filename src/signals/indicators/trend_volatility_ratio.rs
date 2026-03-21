//! Trend Volatility Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Trend Volatility Ratio (TVR).
///
/// Measures the ratio of net directional price movement to total volatility
/// (sum of bar ranges) over a rolling window. Distinguishes trending moves
/// from volatile but directionless chop.
///
/// Formula:
/// - `net_move = close_t - close_{t-period}`
/// - `total_range = Σ(high_i - low_i)` over `period` bars
/// - `tvr = net_move / total_range`
///
/// Range: [−1, +1] when range equals net movement; may exceed ±1 theoretically.
///
/// - Positive: upward trend relative to volatility.
/// - Negative: downward trend relative to volatility.
/// - Near 0: chopping with high volatility but little net progress.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::TrendVolatilityRatio;
/// use fin_primitives::signals::Signal;
/// let tvr = TrendVolatilityRatio::new("tvr_14", 14).unwrap();
/// assert_eq!(tvr.period(), 14);
/// ```
pub struct TrendVolatilityRatio {
    name: String,
    period: usize,
    /// (close, range) per bar
    bars: VecDeque<(Decimal, Decimal)>,
}

impl TrendVolatilityRatio {
    /// Constructs a new `TrendVolatilityRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, bars: VecDeque::with_capacity(period) })
    }
}

impl Signal for TrendVolatilityRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.bars.push_back((bar.close, range));
        if self.bars.len() > self.period {
            self.bars.pop_front();
        }
        if self.bars.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let first_close = self.bars.front().unwrap().0;
        let last_close = self.bars.back().unwrap().0;
        let net_move = last_close - first_close;

        let total_range: Decimal = self.bars.iter().map(|(_, r)| r).copied().sum();

        if total_range.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let tvr = net_move.checked_div(total_range).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(tvr))
    }

    fn is_ready(&self) -> bool {
        self.bars.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.bars.clear();
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
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: cl,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(TrendVolatilityRatio::new("tvr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut tvr = TrendVolatilityRatio::new("tvr", 3).unwrap();
        assert_eq!(tvr.update_bar(&bar("12", "8", "10")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_flat_price_zero_tvr() {
        let mut tvr = TrendVolatilityRatio::new("tvr", 3).unwrap();
        // First close = 10, last close = 10, so net_move = 0
        tvr.update_bar(&bar("12", "8", "10")).unwrap();
        tvr.update_bar(&bar("13", "7", "10")).unwrap();
        let v = tvr.update_bar(&bar("11", "9", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_trending_up_positive() {
        let mut tvr = TrendVolatilityRatio::new("tvr", 3).unwrap();
        tvr.update_bar(&bar("101", "99", "100")).unwrap();
        tvr.update_bar(&bar("103", "101", "102")).unwrap();
        let v = tvr.update_bar(&bar("105", "103", "104")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut tvr = TrendVolatilityRatio::new("tvr", 2).unwrap();
        tvr.update_bar(&bar("12", "8", "10")).unwrap();
        tvr.update_bar(&bar("12", "8", "10")).unwrap();
        assert!(tvr.is_ready());
        tvr.reset();
        assert!(!tvr.is_ready());
    }
}
