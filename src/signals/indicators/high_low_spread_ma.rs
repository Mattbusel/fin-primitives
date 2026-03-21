//! High-Low Spread MA indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Low Spread MA.
///
/// Rolling simple moving average of the intrabar high-low spread expressed
/// as a percentage of the midpoint price. Represents average percentage
/// range (spread) over the window.
///
/// Per-bar formula:
/// - `mid = (high + low) / 2`
/// - `spread_pct = (high - low) / mid * 100` (0 when mid == 0)
///
/// Rolling: `mean(spread_pct, period)`
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowSpreadMa;
/// use fin_primitives::signals::Signal;
/// let hlsm = HighLowSpreadMa::new("hlsm_14", 14).unwrap();
/// assert_eq!(hlsm.period(), 14);
/// ```
pub struct HighLowSpreadMa {
    name: String,
    period: usize,
    spreads: VecDeque<Decimal>,
}

impl HighLowSpreadMa {
    /// Constructs a new `HighLowSpreadMa`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, spreads: VecDeque::with_capacity(period) })
    }
}

impl Signal for HighLowSpreadMa {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = (bar.high + bar.low)
            .checked_div(Decimal::TWO)
            .ok_or(FinError::ArithmeticOverflow)?;

        let spread_pct = if mid.is_zero() {
            Decimal::ZERO
        } else {
            (bar.high - bar.low)
                .checked_div(mid)
                .ok_or(FinError::ArithmeticOverflow)?
                .checked_mul(Decimal::from(100u32))
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.spreads.push_back(spread_pct);
        if self.spreads.len() > self.period {
            self.spreads.pop_front();
        }
        if self.spreads.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.spreads.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.spreads.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.spreads.clear();
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
        assert!(matches!(HighLowSpreadMa::new("hlsm", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut hlsm = HighLowSpreadMa::new("hlsm", 3).unwrap();
        assert_eq!(hlsm.update_bar(&bar("12", "8")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_flat_bar_zero_spread() {
        // high=low → spread=0
        let mut hlsm = HighLowSpreadMa::new("hlsm", 3).unwrap();
        for _ in 0..3 {
            hlsm.update_bar(&bar("10", "10")).unwrap();
        }
        let v = hlsm.update_bar(&bar("10", "10")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_known_spread() {
        // high=12, low=8 → mid=10, spread=(4/10)*100=40%
        let mut hlsm = HighLowSpreadMa::new("hlsm", 3).unwrap();
        for _ in 0..3 {
            hlsm.update_bar(&bar("12", "8")).unwrap();
        }
        let v = hlsm.update_bar(&bar("12", "8")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(40)));
    }

    #[test]
    fn test_reset() {
        let mut hlsm = HighLowSpreadMa::new("hlsm", 2).unwrap();
        hlsm.update_bar(&bar("12", "8")).unwrap();
        hlsm.update_bar(&bar("12", "8")).unwrap();
        assert!(hlsm.is_ready());
        hlsm.reset();
        assert!(!hlsm.is_ready());
    }
}
