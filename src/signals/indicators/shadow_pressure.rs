//! Shadow Pressure indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Shadow Pressure.
///
/// Measures the cumulative directional pressure from wicks over a rolling window.
/// Each bar contributes: upper wick pressure (bearish, wicks represent rejection) and
/// lower wick pressure (bullish, lower wicks represent support/buying).
///
/// Formula per bar: `pressure = (lower_wick − upper_wick) / range`
/// Rolling: `shadow_pressure = mean(pressure, period)`
///
/// - Positive: consistent lower wick dominance → bullish rejection/support.
/// - Negative: consistent upper wick dominance → bearish rejection/resistance.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ShadowPressure;
/// use fin_primitives::signals::Signal;
/// let sp = ShadowPressure::new("sp_14", 14).unwrap();
/// assert_eq!(sp.period(), 14);
/// ```
pub struct ShadowPressure {
    name: String,
    period: usize,
    pressures: VecDeque<Decimal>,
}

impl ShadowPressure {
    /// Constructs a new `ShadowPressure` with the given name and period.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { name: name.into(), period, pressures: VecDeque::with_capacity(period) })
    }
}

impl Signal for ShadowPressure {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        let pressure = if range.is_zero() {
            Decimal::ZERO
        } else {
            let upper_wick = bar.high - bar.close.max(bar.open);
            let lower_wick = bar.close.min(bar.open) - bar.low;
            (lower_wick - upper_wick)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.pressures.push_back(pressure);
        if self.pressures.len() > self.period {
            self.pressures.pop_front();
        }
        if self.pressures.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let sum: Decimal = self.pressures.iter().copied().sum();
        #[allow(clippy::cast_possible_truncation)]
        let avg = sum
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(avg))
    }

    fn is_ready(&self) -> bool {
        self.pressures.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.pressures.clear();
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
    fn test_period_zero_fails() {
        assert!(matches!(ShadowPressure::new("sp", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut sp = ShadowPressure::new("sp", 3).unwrap();
        assert_eq!(sp.update_bar(&bar("10", "12", "9", "11")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_lower_wick_dominant_positive() {
        let mut sp = ShadowPressure::new("sp", 3).unwrap();
        // Each bar: lower_wick > upper_wick → positive pressure
        for _ in 0..3 {
            // open=15, high=16, low=10, close=15 → lower_wick=5, upper_wick=1
            sp.update_bar(&bar("15", "16", "10", "15")).unwrap();
        }
        let v = sp.update_bar(&bar("15", "16", "10", "15")).unwrap();
        if let SignalValue::Scalar(s) = v { assert!(s > dec!(0)); }
        else { panic!("expected scalar"); }
    }

    #[test]
    fn test_upper_wick_dominant_negative() {
        let mut sp = ShadowPressure::new("sp", 3).unwrap();
        // Each bar: upper_wick > lower_wick → negative pressure
        for _ in 0..3 {
            // open=15, high=20, low=14, close=15 → lower_wick=1, upper_wick=5
            sp.update_bar(&bar("15", "20", "14", "15")).unwrap();
        }
        let v = sp.update_bar(&bar("15", "20", "14", "15")).unwrap();
        if let SignalValue::Scalar(s) = v { assert!(s < dec!(0)); }
        else { panic!("expected scalar"); }
    }

    #[test]
    fn test_reset() {
        let mut sp = ShadowPressure::new("sp", 2).unwrap();
        sp.update_bar(&bar("15", "16", "10", "15")).unwrap();
        sp.update_bar(&bar("15", "16", "10", "15")).unwrap();
        assert!(sp.is_ready());
        sp.reset();
        assert!(!sp.is_ready());
    }
}
