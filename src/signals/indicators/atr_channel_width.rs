//! ATR Channel Width indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// ATR Channel Width.
///
/// Computes the width of a symmetric ATR-based channel around the rolling
/// midpoint price, expressed as a percentage of the midpoint.
///
/// The ATR here uses simple range averaging (`mean(high - low, period)`).
/// Channel width = `2 * ATR` as percentage of mean midpoint.
///
/// Formula:
/// - `atr = mean(high - low, period)`
/// - `mid = mean((high + low) / 2, period)`
/// - `channel_width_pct = 2 * atr / mid * 100` (0 when mid == 0)
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::AtrChannelWidth;
/// use fin_primitives::signals::Signal;
/// let acw = AtrChannelWidth::new("acw_14", 14).unwrap();
/// assert_eq!(acw.period(), 14);
/// ```
pub struct AtrChannelWidth {
    name: String,
    period: usize,
    /// (range, midpoint) per bar
    bars: VecDeque<(Decimal, Decimal)>,
}

impl AtrChannelWidth {
    /// Constructs a new `AtrChannelWidth`.
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

impl Signal for AtrChannelWidth {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let mid = (bar.high + bar.low)
            .checked_div(Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;

        self.bars.push_back((range, mid));
        if self.bars.len() > self.period {
            self.bars.pop_front();
        }
        if self.bars.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let range_sum: Decimal = self.bars.iter().map(|(r, _)| r).copied().sum();
        let mid_sum: Decimal = self.bars.iter().map(|(_, m)| m).copied().sum();

        #[allow(clippy::cast_possible_truncation)]
        let n = Decimal::from(self.period as u32);
        let atr = range_sum.checked_div(n).ok_or(FinError::ArithmeticOverflow)?;
        let mean_mid = mid_sum.checked_div(n).ok_or(FinError::ArithmeticOverflow)?;

        if mean_mid.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let channel_pct = (Decimal::TWO * atr)
            .checked_div(mean_mid)
            .ok_or(FinError::ArithmeticOverflow)?
            .checked_mul(Decimal::from(100u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        Ok(SignalValue::Scalar(channel_pct))
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hi = Price::new(h.parse().unwrap()).unwrap();
        let lo = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lo, high: hi, low: lo, close: hi,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(AtrChannelWidth::new("acw", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut acw = AtrChannelWidth::new("acw", 3).unwrap();
        assert_eq!(acw.update_bar(&bar("12", "8")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_known_channel_width() {
        // high=12, low=8 → range=4, mid=10, atr=4, channel=2*4/10*100=80%
        let mut acw = AtrChannelWidth::new("acw", 3).unwrap();
        for _ in 0..3 {
            acw.update_bar(&bar("12", "8")).unwrap();
        }
        let v = acw.update_bar(&bar("12", "8")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(80)));
    }

    #[test]
    fn test_flat_bar_zero_width() {
        let mut acw = AtrChannelWidth::new("acw", 3).unwrap();
        for _ in 0..3 {
            acw.update_bar(&bar("10", "10")).unwrap();
        }
        let v = acw.update_bar(&bar("10", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_reset() {
        let mut acw = AtrChannelWidth::new("acw", 2).unwrap();
        acw.update_bar(&bar("12", "8")).unwrap();
        acw.update_bar(&bar("12", "8")).unwrap();
        assert!(acw.is_ready());
        acw.reset();
        assert!(!acw.is_ready());
    }
}
