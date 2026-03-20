//! High-Low Crossover indicator -- when close crosses the period high or low.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High-Low Crossover -- detects when close breaks to a new `period`-bar high or low.
///
/// Returns:
/// - `+1` if `close[t] == max(high, period)` (new period high)
/// - `-1` if `close[t] == min(low, period)`  (new period low)
/// - `0`  otherwise
///
/// Useful as a breakout signal: a new high or low often precedes continued movement.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighLowCrossover;
/// use fin_primitives::signals::Signal;
/// let hlx = HighLowCrossover::new("hlx", 20).unwrap();
/// assert_eq!(hlx.period(), 20);
/// ```
pub struct HighLowCrossover {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl HighLowCrossover {
    /// Constructs a new `HighLowCrossover`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            highs: VecDeque::with_capacity(period),
            lows: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for HighLowCrossover {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.highs.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period { return Ok(SignalValue::Unavailable); }
        let period_high = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
        let period_low  = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
        let signal = if bar.close >= period_high {
            Decimal::ONE
        } else if bar.close <= period_low {
            Decimal::NEGATIVE_ONE
        } else {
            Decimal::ZERO
        };
        Ok(SignalValue::Scalar(signal))
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
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
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: cp, high: hp, low: lp, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_hlx_period_0_error() { assert!(HighLowCrossover::new("hlx", 0).is_err()); }

    #[test]
    fn test_hlx_unavailable_before_period() {
        let mut hlx = HighLowCrossover::new("hlx", 3).unwrap();
        assert_eq!(hlx.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_hlx_new_high_returns_plus1() {
        let mut hlx = HighLowCrossover::new("hlx", 3).unwrap();
        hlx.update_bar(&bar("105", "95", "100")).unwrap();
        hlx.update_bar(&bar("107", "93", "105")).unwrap();
        // period_high = 110, close = 110 >= 110 -> +1
        let v = hlx.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_hlx_new_low_returns_minus1() {
        let mut hlx = HighLowCrossover::new("hlx", 3).unwrap();
        hlx.update_bar(&bar("105", "95", "100")).unwrap();
        hlx.update_bar(&bar("107", "93", "95")).unwrap();
        // period_low = 90, close = 90 <= 90 -> -1
        let v = hlx.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_hlx_middle_returns_zero() {
        let mut hlx = HighLowCrossover::new("hlx", 3).unwrap();
        hlx.update_bar(&bar("110", "90", "100")).unwrap();
        hlx.update_bar(&bar("110", "90", "100")).unwrap();
        // period_high=110, period_low=90, close=100 -> 0
        let v = hlx.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_hlx_reset() {
        let mut hlx = HighLowCrossover::new("hlx", 2).unwrap();
        hlx.update_bar(&bar("110", "90", "100")).unwrap();
        hlx.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(hlx.is_ready());
        hlx.reset();
        assert!(!hlx.is_ready());
    }
}
