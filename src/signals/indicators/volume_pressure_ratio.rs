//! Volume Pressure Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Pressure Ratio.
///
/// Separates bullish (close > open) and bearish (close < open) volume over a
/// rolling window, then computes their ratio. Neutral/doji bars split volume
/// equally between the two sides.
///
/// Formula:
/// - `bull_vol` = rolling sum of volume on bullish bars
/// - `bear_vol` = rolling sum of volume on bearish bars
/// - `ratio = bull_vol / (bull_vol + bear_vol)` ∈ [0, 1]
///
/// Values > 0.5 indicate buying pressure dominates; < 0.5 indicates selling
/// pressure. Returns `0.5` when both sides are zero (no volume in window).
///
/// Returns `SignalValue::Unavailable` until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumePressureRatio;
/// use fin_primitives::signals::Signal;
/// let vpr = VolumePressureRatio::new("vpr_20", 20).unwrap();
/// assert_eq!(vpr.period(), 20);
/// ```
pub struct VolumePressureRatio {
    name: String,
    period: usize,
    bull_vols: VecDeque<Decimal>,
    bear_vols: VecDeque<Decimal>,
}

impl VolumePressureRatio {
    /// Constructs a new `VolumePressureRatio` with the given name and period.
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
            bull_vols: VecDeque::with_capacity(period),
            bear_vols: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for VolumePressureRatio {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let half_vol = bar.volume
            .checked_div(Decimal::from(2u32))
            .ok_or(FinError::ArithmeticOverflow)?;

        let (bull, bear) = if bar.close > bar.open {
            (bar.volume, Decimal::ZERO)
        } else if bar.close < bar.open {
            (Decimal::ZERO, bar.volume)
        } else {
            // Doji: split evenly
            (half_vol, half_vol)
        };

        self.bull_vols.push_back(bull);
        self.bear_vols.push_back(bear);
        if self.bull_vols.len() > self.period {
            self.bull_vols.pop_front();
            self.bear_vols.pop_front();
        }

        if self.bull_vols.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let total_bull: Decimal = self.bull_vols.iter().copied().sum();
        let total_bear: Decimal = self.bear_vols.iter().copied().sum();
        let total = total_bull + total_bear;

        if total.is_zero() {
            return Ok(SignalValue::Scalar(Decimal::new(5, 1))); // 0.5
        }

        let ratio = total_bull
            .checked_div(total)
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool {
        self.bull_vols.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
    }

    fn reset(&mut self) {
        self.bull_vols.clear();
        self.bear_vols.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, close: &str, vol: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o,
            high: c.max(o),
            low: c.min(o),
            close: c,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(VolumePressureRatio::new("vpr", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut vpr = VolumePressureRatio::new("vpr", 3).unwrap();
        let v = vpr.update_bar(&bar("10", "12", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_all_bullish_ratio_one() {
        let mut vpr = VolumePressureRatio::new("vpr", 3).unwrap();
        for _ in 0..3 {
            vpr.update_bar(&bar("10", "12", "100")).unwrap();
        }
        let v = vpr.update_bar(&bar("10", "12", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_all_bearish_ratio_zero() {
        let mut vpr = VolumePressureRatio::new("vpr", 3).unwrap();
        for _ in 0..3 {
            vpr.update_bar(&bar("12", "10", "100")).unwrap();
        }
        let v = vpr.update_bar(&bar("12", "10", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_equal_bull_bear_half() {
        let mut vpr = VolumePressureRatio::new("vpr", 2).unwrap();
        vpr.update_bar(&bar("10", "12", "100")).unwrap(); // bull
        let v = vpr.update_bar(&bar("12", "10", "100")).unwrap(); // bear
        assert_eq!(v, SignalValue::Scalar(dec!(0.5)));
    }

    #[test]
    fn test_reset() {
        let mut vpr = VolumePressureRatio::new("vpr", 2).unwrap();
        vpr.update_bar(&bar("10", "12", "100")).unwrap();
        vpr.update_bar(&bar("12", "10", "100")).unwrap();
        assert!(vpr.is_ready());
        vpr.reset();
        assert!(!vpr.is_ready());
    }
}
