//! Open-Close Pressure indicator.
//!
//! Measures the directional pressure from the opening gap and the intrabar
//! move, combining both into a single normalized pressure score.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Open-Close Pressure: rolling sum of `(close - open) / range` over `period` bars.
///
/// Each bar contributes a value in `[-1, +1]` (or 0 for flat/doji bars):
///
/// ```text
/// bar_pressure = (close - open) / (high - low)   when high > low
///              = 0                                when high == low
/// ```
///
/// The rolling `period`-bar sum aggregates directional pressure, ranging from
/// `-period` (all bars fully bearish) to `+period` (all bars fully bullish).
/// A value near zero indicates balanced buying and selling pressure.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenClosePressure;
/// use fin_primitives::signals::Signal;
///
/// let ocp = OpenClosePressure::new("ocp", 10).unwrap();
/// assert_eq!(ocp.period(), 10);
/// assert!(!ocp.is_ready());
/// ```
pub struct OpenClosePressure {
    name: String,
    period: usize,
    window: std::collections::VecDeque<Decimal>,
    running_sum: Decimal,
}

impl OpenClosePressure {
    /// Constructs a new `OpenClosePressure`.
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
            window: std::collections::VecDeque::with_capacity(period),
            running_sum: Decimal::ZERO,
        })
    }
}

impl Signal for OpenClosePressure {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.window.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let pressure = if range.is_zero() {
            Decimal::ZERO
        } else {
            (bar.close - bar.open)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.running_sum += pressure;
        self.window.push_back(pressure);

        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() {
                self.running_sum -= old;
            }
        }

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        Ok(SignalValue::Scalar(self.running_sum))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.running_sum = Decimal::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, high: &str, low: &str, close: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(open.parse().unwrap()).unwrap(),
            high: Price::new(high.parse().unwrap()).unwrap(),
            low: Price::new(low.parse().unwrap()).unwrap(),
            close: Price::new(close.parse().unwrap()).unwrap(),
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ocp_invalid_period() {
        assert!(OpenClosePressure::new("ocp", 0).is_err());
    }

    #[test]
    fn test_ocp_unavailable_before_period() {
        let mut ocp = OpenClosePressure::new("ocp", 3).unwrap();
        for _ in 0..2 {
            assert_eq!(
                ocp.update_bar(&bar("100", "110", "90", "105")).unwrap(),
                SignalValue::Unavailable
            );
        }
    }

    #[test]
    fn test_ocp_all_bullish_bars_positive() {
        let mut ocp = OpenClosePressure::new("ocp", 3).unwrap();
        // Each bar: open at low, close at high → pressure = +1
        for _ in 0..3 {
            ocp.update_bar(&bar("90", "110", "90", "110")).unwrap();
        }
        let v = ocp.update_bar(&bar("90", "110", "90", "110")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0), "all bullish bars → positive pressure: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ocp_all_bearish_bars_negative() {
        let mut ocp = OpenClosePressure::new("ocp", 3).unwrap();
        // Each bar: open at high, close at low → pressure = -1
        for _ in 0..3 {
            ocp.update_bar(&bar("110", "110", "90", "90")).unwrap();
        }
        let v = ocp.update_bar(&bar("110", "110", "90", "90")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s < dec!(0), "all bearish bars → negative pressure: {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ocp_doji_bars_zero() {
        let mut ocp = OpenClosePressure::new("ocp", 3).unwrap();
        // Doji: open == close → pressure = 0 per bar
        for _ in 0..3 {
            ocp.update_bar(&bar("100", "110", "90", "100")).unwrap();
        }
        let v = ocp.update_bar(&bar("100", "110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ocp_flat_bar_pressure_zero() {
        let mut ocp = OpenClosePressure::new("ocp", 2).unwrap();
        ocp.update_bar(&bar("100", "100", "100", "100")).unwrap();
        let v = ocp.update_bar(&bar("100", "100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ocp_reset() {
        let mut ocp = OpenClosePressure::new("ocp", 2).unwrap();
        ocp.update_bar(&bar("100", "110", "90", "105")).unwrap();
        ocp.update_bar(&bar("100", "110", "90", "105")).unwrap();
        assert!(ocp.is_ready());
        ocp.reset();
        assert!(!ocp.is_ready());
    }
}
