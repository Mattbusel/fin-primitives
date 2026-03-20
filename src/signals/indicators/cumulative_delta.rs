//! Cumulative Volume Delta indicator -- rolling net volume delta.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Cumulative Delta -- rolling signed volume sum over the last `period` bars.
///
/// Each bar contributes:
/// - `+volume` if close > open (up-bar / buying pressure)
/// - `-volume` if close < open (down-bar / selling pressure)
/// - `0` if close == open (doji / neutral)
///
/// ```text
/// delta[t]     = volume[t]  if close > open
///              = -volume[t] if close < open
///              = 0          if close == open
/// cum_delta[t] = sum(delta, period)
/// ```
///
/// Positive values indicate net buying pressure; negative values indicate net selling.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CumulativeDelta;
/// use fin_primitives::signals::Signal;
/// let cd = CumulativeDelta::new("cd", 10).unwrap();
/// assert_eq!(cd.period(), 10);
/// ```
pub struct CumulativeDelta {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CumulativeDelta {
    /// Constructs a new `CumulativeDelta`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for CumulativeDelta {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let delta = if bar.close > bar.open {
            bar.volume
        } else if bar.close < bar.open {
            -bar.volume
        } else {
            Decimal::ZERO
        };
        self.window.push_back(delta);
        self.sum += delta;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(self.sum))
    }

    fn reset(&mut self) {
        self.window.clear();
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

    fn bar(o: &str, c: &str, vol: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let v = Quantity::new(vol.parse().unwrap()).unwrap();
        let high = if cp.value() > op.value() { cp } else { op };
        let low  = if cp.value() < op.value() { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high, low, close: cp, volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_cd_period_0_error() { assert!(CumulativeDelta::new("cd", 0).is_err()); }

    #[test]
    fn test_cd_unavailable_before_period() {
        let mut cd = CumulativeDelta::new("cd", 3).unwrap();
        assert_eq!(cd.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cd_all_up_bars() {
        let mut cd = CumulativeDelta::new("cd", 3).unwrap();
        cd.update_bar(&bar("100", "105", "1000")).unwrap();
        cd.update_bar(&bar("100", "105", "2000")).unwrap();
        let v = cd.update_bar(&bar("100", "105", "3000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(6000)));
    }

    #[test]
    fn test_cd_all_down_bars() {
        let mut cd = CumulativeDelta::new("cd", 3).unwrap();
        cd.update_bar(&bar("105", "100", "1000")).unwrap();
        cd.update_bar(&bar("105", "100", "2000")).unwrap();
        let v = cd.update_bar(&bar("105", "100", "3000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-6000)));
    }

    #[test]
    fn test_cd_mixed_nets_zero() {
        let mut cd = CumulativeDelta::new("cd", 2).unwrap();
        cd.update_bar(&bar("100", "105", "1000")).unwrap(); // +1000
        let v = cd.update_bar(&bar("105", "100", "1000")).unwrap(); // -1000, sum=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cd_window_slides() {
        let mut cd = CumulativeDelta::new("cd", 2).unwrap();
        cd.update_bar(&bar("100", "105", "1000")).unwrap(); // +1000, not ready
        cd.update_bar(&bar("100", "105", "2000")).unwrap(); // +2000, sum=3000
        let v = cd.update_bar(&bar("100", "105", "500")).unwrap(); // +500, drop 1000 -> 2500
        assert_eq!(v, SignalValue::Scalar(dec!(2500)));
    }

    #[test]
    fn test_cd_reset() {
        let mut cd = CumulativeDelta::new("cd", 2).unwrap();
        cd.update_bar(&bar("100", "105", "1000")).unwrap();
        cd.update_bar(&bar("100", "105", "1000")).unwrap();
        assert!(cd.is_ready());
        cd.reset();
        assert!(!cd.is_ready());
    }
}
