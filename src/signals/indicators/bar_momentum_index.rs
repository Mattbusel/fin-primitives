//! Bar Momentum Index — fraction of last N bars aligned with the current bar's direction.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bar Momentum Index — measures directional consensus among recent bars.
///
/// On each bar, determines whether the bar is **up** (`close > open`), **down** (`close < open`),
/// or **flat** (`close == open`). Then returns the fraction of the last `period` bars
/// that moved in the *same* direction as the current bar.
///
/// - If today is up: output = fraction of last `period` bars that were also up.
/// - If today is down: output = fraction of last `period` bars that were also down.
/// - If today is flat: output = `0` (no directional consensus to measure).
///
/// A high value (near 1) indicates strong directional persistence.
/// A low value (near 0) indicates the current bar is going against the recent trend.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarMomentumIndex;
/// use fin_primitives::signals::Signal;
/// let bmi = BarMomentumIndex::new("bmi_10", 10).unwrap();
/// assert_eq!(bmi.period(), 10);
/// ```
pub struct BarMomentumIndex {
    name: String,
    period: usize,
    /// 1 = up bar, -1 = down bar, 0 = flat.
    directions: VecDeque<i8>,
}

impl BarMomentumIndex {
    /// Constructs a new `BarMomentumIndex`.
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
            directions: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for BarMomentumIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn period(&self) -> usize {
        self.period
    }

    fn is_ready(&self) -> bool {
        self.directions.len() >= self.period
    }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let dir: i8 = if bar.is_bullish() {
            1
        } else if bar.is_bearish() {
            -1
        } else {
            0
        };

        self.directions.push_back(dir);
        if self.directions.len() > self.period {
            self.directions.pop_front();
        }

        if self.directions.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        if dir == 0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }

        let aligned = self.directions.iter().filter(|&&d| d == dir).count();
        #[allow(clippy::cast_possible_truncation)]
        let ratio = Decimal::from(aligned as u32)
            .checked_div(Decimal::from(self.period as u32))
            .ok_or(FinError::ArithmeticOverflow)?;
        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.directions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(open: &str, close: &str) -> OhlcvBar {
        let o = Price::new(open.parse().unwrap()).unwrap();
        let c = Price::new(close.parse().unwrap()).unwrap();
        let h = if o.value() >= c.value() { o } else { c };
        let l = if o.value() <= c.value() { o } else { c };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: o, high: h, low: l, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bmi_invalid_period() {
        assert!(BarMomentumIndex::new("bmi", 0).is_err());
    }

    #[test]
    fn test_bmi_unavailable_before_period() {
        let mut bmi = BarMomentumIndex::new("bmi", 3).unwrap();
        assert_eq!(bmi.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        assert_eq!(bmi.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
        assert!(!bmi.is_ready());
    }

    #[test]
    fn test_bmi_all_aligned_up_gives_one() {
        let mut bmi = BarMomentumIndex::new("bmi", 3).unwrap();
        bmi.update_bar(&bar("100", "105")).unwrap();
        bmi.update_bar(&bar("105", "110")).unwrap();
        bmi.update_bar(&bar("110", "115")).unwrap();
        let v = bmi.update_bar(&bar("115", "120")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_bmi_all_opposite_gives_low_value() {
        // 3 up bars, then a down bar → 0/3 = 0 down bars in window.
        let mut bmi = BarMomentumIndex::new("bmi", 3).unwrap();
        bmi.update_bar(&bar("100", "105")).unwrap();
        bmi.update_bar(&bar("105", "110")).unwrap();
        bmi.update_bar(&bar("110", "115")).unwrap();
        // Now today is DOWN — none of the last 3 were also down.
        let v = bmi.update_bar(&bar("115", "110")).unwrap();
        // Window is [up, up, down] — today=down, count of down = 1, ratio = 1/3
        if let SignalValue::Scalar(r) = v {
            assert!(r < dec!(0.5), "expected low value, got {r}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_bmi_flat_bar_gives_zero() {
        let mut bmi = BarMomentumIndex::new("bmi", 3).unwrap();
        bmi.update_bar(&bar("100", "105")).unwrap();
        bmi.update_bar(&bar("105", "110")).unwrap();
        bmi.update_bar(&bar("110", "115")).unwrap();
        let v = bmi.update_bar(&bar("115", "115")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bmi_output_in_unit_interval() {
        let mut bmi = BarMomentumIndex::new("bmi", 5).unwrap();
        let bars = [
            bar("100", "105"), bar("105", "102"), bar("102", "108"),
            bar("108", "106"), bar("106", "110"), bar("110", "107"),
        ];
        for b in &bars {
            if let SignalValue::Scalar(v) = bmi.update_bar(b).unwrap() {
                assert!(v >= dec!(0));
                assert!(v <= dec!(1));
            }
        }
    }

    #[test]
    fn test_bmi_reset() {
        let mut bmi = BarMomentumIndex::new("bmi", 2).unwrap();
        bmi.update_bar(&bar("100", "105")).unwrap();
        bmi.update_bar(&bar("105", "110")).unwrap();
        assert!(bmi.is_ready());
        bmi.reset();
        assert!(!bmi.is_ready());
    }

    #[test]
    fn test_bmi_period_and_name() {
        let bmi = BarMomentumIndex::new("my_bmi", 10).unwrap();
        assert_eq!(bmi.period(), 10);
        assert_eq!(bmi.name(), "my_bmi");
    }
}
