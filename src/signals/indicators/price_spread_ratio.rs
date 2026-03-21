//! Price Spread Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Spread Ratio.
///
/// Measures the ratio of the rolling high-low spread to the rolling mean close price,
/// giving a normalized measure of how wide the price range is relative to the price level.
///
/// Formula:
/// - `period_high = max(high, period)`
/// - `period_low = min(low, period)`
/// - `mean_close = mean(close, period)`
/// - `psr = (period_high - period_low) / mean_close`
///
/// Higher values indicate wider price range relative to price level.
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
/// Returns `SignalValue::Scalar(0.0)` when mean close is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceSpreadRatio;
/// use fin_primitives::signals::Signal;
/// let psr = PriceSpreadRatio::new("psr_20", 20).unwrap();
/// assert_eq!(psr.period(), 20);
/// ```
pub struct PriceSpreadRatio {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    closes: VecDeque<Decimal>,
}

impl PriceSpreadRatio {
    /// Constructs a new `PriceSpreadRatio`.
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
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
            closes: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for PriceSpreadRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        self.closes.push_back(bar.close);

        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
            self.closes.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let period_high = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let period_low = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let spread = period_high - period_low;

        let close_sum: Decimal = self.closes.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let mean_close = close_sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        if mean_close.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let psr = spread.checked_div(mean_close).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(psr))
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.closes.clear();
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
        assert!(matches!(PriceSpreadRatio::new("psr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut psr = PriceSpreadRatio::new("psr", 3).unwrap();
        assert_eq!(psr.update_bar(&bar("12", "10", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_identical_bars_gives_zero_spread() {
        // All bars same: high=low=close → spread=0 → psr=0
        let mut psr = PriceSpreadRatio::new("psr", 3).unwrap();
        for _ in 0..3 {
            psr.update_bar(&bar("100", "100", "100")).unwrap();
        }
        let v = psr.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_spread_ratio_positive() {
        let mut psr = PriceSpreadRatio::new("psr", 3).unwrap();
        for _ in 0..3 {
            psr.update_bar(&bar("110", "90", "100")).unwrap();
        }
        let v = psr.update_bar(&bar("110", "90", "100")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut psr = PriceSpreadRatio::new("psr", 2).unwrap();
        psr.update_bar(&bar("12", "10", "11")).unwrap();
        psr.update_bar(&bar("12", "10", "11")).unwrap();
        assert!(psr.is_ready());
        psr.reset();
        assert!(!psr.is_ready());
    }
}
