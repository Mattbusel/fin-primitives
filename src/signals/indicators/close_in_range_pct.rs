//! Close-in-Range Percentage indicator.
//!
//! Rolling mean of the close's position within each bar's high-low range,
//! expressed as a percentage.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close-in-Range Percentage — rolling mean of `(close - low) / (high - low) × 100`.
///
/// For each bar:
/// ```text
/// pct[i]  = (close[i] - low[i]) / (high[i] - low[i]) × 100   when high > low
///         = 50                                                 when high == low (flat bar)
/// ```
///
/// This ranges from `0` (close at the low) to `100` (close at the high).
/// The rolling mean reveals the typical intrabar close bias:
/// - **Near 100**: market habitually closes near the session high — bullish
///   absorption and buying into the close.
/// - **Near 0**: closes persistently near the session low — bearish distribution.
/// - **Near 50**: balanced — no intrabar directional bias.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars are collected.
///
/// # Errors
/// Returns [`FinError::InvalidPeriod`] if `period == 0`.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseInRangePct;
/// use fin_primitives::signals::Signal;
/// let cirp = CloseInRangePct::new("cirp_14", 14).unwrap();
/// assert_eq!(cirp.period(), 14);
/// ```
pub struct CloseInRangePct {
    name: String,
    period: usize,
    values: VecDeque<Decimal>,
    sum: Decimal,
}

impl CloseInRangePct {
    /// Constructs a new `CloseInRangePct`.
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

impl Signal for CloseInRangePct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.values.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.range();
        let hundred = Decimal::from(100u32);
        let pct = if range.is_zero() {
            Decimal::from(50u32) // midpoint for flat bars
        } else {
            (bar.close - bar.low)
                .checked_div(range)
                .ok_or(FinError::ArithmeticOverflow)?
                .checked_mul(hundred)
                .ok_or(FinError::ArithmeticOverflow)?
        };

        self.sum += pct;
        self.values.push_back(pct);
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

    fn bar(h: &str, l: &str, c: &str) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: Price::new(l.parse().unwrap()).unwrap(),
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
    fn test_cirp_invalid_period() {
        assert!(CloseInRangePct::new("cirp", 0).is_err());
    }

    #[test]
    fn test_cirp_unavailable_during_warmup() {
        let mut cirp = CloseInRangePct::new("cirp", 3).unwrap();
        assert_eq!(cirp.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(cirp.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert!(!cirp.is_ready());
    }

    #[test]
    fn test_cirp_close_at_high_100() {
        // close = high → pct = 100
        let mut cirp = CloseInRangePct::new("cirp", 3).unwrap();
        for _ in 0..3 { cirp.update_bar(&bar("110", "90", "110")).unwrap(); }
        if let SignalValue::Scalar(v) = cirp.update_bar(&bar("110", "90", "110")).unwrap() {
            assert_eq!(v, dec!(100));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cirp_close_at_low_zero() {
        let mut cirp = CloseInRangePct::new("cirp", 3).unwrap();
        for _ in 0..3 { cirp.update_bar(&bar("110", "90", "90")).unwrap(); }
        if let SignalValue::Scalar(v) = cirp.update_bar(&bar("110", "90", "90")).unwrap() {
            assert_eq!(v, dec!(0));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cirp_close_at_midpoint_50() {
        let mut cirp = CloseInRangePct::new("cirp", 2).unwrap();
        cirp.update_bar(&bar("110", "90", "100")).unwrap();
        if let SignalValue::Scalar(v) = cirp.update_bar(&bar("110", "90", "100")).unwrap() {
            assert_eq!(v, dec!(50));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cirp_flat_bar_50() {
        let mut cirp = CloseInRangePct::new("cirp", 2).unwrap();
        cirp.update_bar(&bar("100", "100", "100")).unwrap();
        if let SignalValue::Scalar(v) = cirp.update_bar(&bar("100", "100", "100")).unwrap() {
            assert_eq!(v, dec!(50));
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_cirp_reset() {
        let mut cirp = CloseInRangePct::new("cirp", 2).unwrap();
        cirp.update_bar(&bar("110", "90", "105")).unwrap();
        cirp.update_bar(&bar("110", "90", "105")).unwrap();
        assert!(cirp.is_ready());
        cirp.reset();
        assert!(!cirp.is_ready());
    }
}
