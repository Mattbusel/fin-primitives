//! Volume Breakout Score indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Breakout Score.
///
/// Combines volume surge with price move to score potential breakout bars.
/// Both components are normalized over the rolling window.
///
/// Per-bar formula:
/// - `vol_ratio = volume / mean(volume, period)` — volume relative to average
/// - `move_ratio = |close - open| / mean(|close - open|, period)` — body relative to average
/// - `score = vol_ratio * move_ratio`
///
/// - High score (> 1): above-average volume AND above-average price move — potential breakout.
/// - Score near 1: average conditions.
/// - Low score: below-average move or volume.
///
/// Returns `SignalValue::Unavailable` until `period` bars accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeBreakoutScore;
/// use fin_primitives::signals::Signal;
/// let vbs = VolumeBreakoutScore::new("vbs_14", 14).unwrap();
/// assert_eq!(vbs.period(), 14);
/// ```
pub struct VolumeBreakoutScore {
    name: String,
    period: usize,
    /// (volume, abs_body) per bar
    bars: VecDeque<(Decimal, Decimal)>,
}

impl VolumeBreakoutScore {
    /// Constructs a new `VolumeBreakoutScore`.
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

impl Signal for VolumeBreakoutScore {
    fn name(&self) -> &str {
        &self.name
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let abs_body = (bar.close - bar.open).abs();
        self.bars.push_back((bar.volume, abs_body));
        if self.bars.len() > self.period {
            self.bars.pop_front();
        }
        if self.bars.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let vol_sum: Decimal = self.bars.iter().map(|(v, _)| v).copied().sum();
        let body_sum: Decimal = self.bars.iter().map(|(_, b)| b).copied().sum();

        #[allow(clippy::cast_possible_truncation)]
        let n = Decimal::from(self.period as u32);
        let mean_vol = vol_sum.checked_div(n).ok_or(FinError::ArithmeticOverflow)?;
        let mean_body = body_sum.checked_div(n).ok_or(FinError::ArithmeticOverflow)?;

        let vol_ratio = if mean_vol.is_zero() {
            Decimal::ONE
        } else {
            bar.volume.checked_div(mean_vol).ok_or(FinError::ArithmeticOverflow)?
        };

        let move_ratio = if mean_body.is_zero() {
            Decimal::ONE
        } else {
            abs_body.checked_div(mean_body).ok_or(FinError::ArithmeticOverflow)?
        };

        let score = vol_ratio.checked_mul(move_ratio).ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(score))
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

    fn bar(o: &str, c: &str, vol: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cl = Price::new(c.parse().unwrap()).unwrap();
        let hi = op.max(cl);
        let lo = op.min(cl);
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hi, low: lo, close: cl,
            volume: Quantity::new(vol.parse().unwrap()).unwrap(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_period_zero_fails() {
        assert!(matches!(VolumeBreakoutScore::new("vbs", 0), Err(FinError::InvalidPeriod(0))));
    }

    #[test]
    fn test_unavailable_before_period() {
        let mut vbs = VolumeBreakoutScore::new("vbs", 3).unwrap();
        assert_eq!(vbs.update_bar(&bar("10", "12", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_uniform_bars_give_one() {
        // All bars identical → vol_ratio=1, move_ratio=1 → score=1
        let mut vbs = VolumeBreakoutScore::new("vbs", 3).unwrap();
        for _ in 0..3 {
            vbs.update_bar(&bar("10", "12", "1000")).unwrap();
        }
        let v = vbs.update_bar(&bar("10", "12", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_breakout_bar_above_one() {
        // Seed with 3 normal bars, then one with 3x volume and 3x move
        let mut vbs = VolumeBreakoutScore::new("vbs", 4).unwrap();
        vbs.update_bar(&bar("10", "11", "1000")).unwrap();
        vbs.update_bar(&bar("10", "11", "1000")).unwrap();
        vbs.update_bar(&bar("10", "11", "1000")).unwrap();
        let v = vbs.update_bar(&bar("10", "13", "3000")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(1));
        } else {
            panic!("expected scalar");
        }
    }

    #[test]
    fn test_reset() {
        let mut vbs = VolumeBreakoutScore::new("vbs", 2).unwrap();
        vbs.update_bar(&bar("10", "12", "1000")).unwrap();
        vbs.update_bar(&bar("10", "12", "1000")).unwrap();
        assert!(vbs.is_ready());
        vbs.reset();
        assert!(!vbs.is_ready());
    }
}
