//! Bars-Since-High/Low indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bars Since High/Low — counts how many bars ago the `n`-bar high and low were set.
///
/// ```text
/// bars_since_high = bars elapsed since the highest high in the n-bar window was recorded
/// bars_since_low  = bars elapsed since the lowest low in the n-bar window was recorded
/// output = bars_since_high - bars_since_low
/// ```
///
/// * Positive output: the low was set more recently than the high (recent bearish momentum).
/// * Negative output: the high was set more recently (recent bullish momentum).
/// * Zero: both set the same bar ago.
///
/// Returns [`SignalValue::Unavailable`] until `n` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarsSince;
/// use fin_primitives::signals::Signal;
///
/// let bs = BarsSince::new("bs", 10).unwrap();
/// assert_eq!(bs.period(), 10);
/// ```
pub struct BarsSince {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl BarsSince {
    /// Creates a new `BarsSince`.
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

    /// Returns `(bars_since_high, bars_since_low)` within the current window.
    ///
    /// Returns `None` if fewer than `period` bars have been seen.
    pub fn bars_since(&self) -> Option<(usize, usize)> {
        if self.highs.len() < self.period { return None; }
        let max_high = self.highs.iter().cloned().fold(Decimal::MIN, Decimal::max);
        let min_low  = self.lows.iter().cloned().fold(Decimal::MAX, Decimal::min);
        // Find last occurrence (rightmost = most recent = smallest bars-ago)
        let bsh = self.highs.iter().rev().position(|&h| h == max_high)?;
        let bsl = self.lows.iter().rev().position(|&l| l == min_low)?;
        Some((bsh, bsl))
    }
}

impl Signal for BarsSince {
    fn name(&self) -> &str { &self.name }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.highs.push_back(bar.high);
        self.lows.push_back(bar.low);
        if self.highs.len() > self.period {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        if self.highs.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let (bsh, bsl) = match self.bars_since() {
            None => return Ok(SignalValue::Unavailable),
            Some(pair) => pair,
        };

        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(
            Decimal::from(bsh as i64) - Decimal::from(bsl as i64)
        ))
    }

    fn is_ready(&self) -> bool {
        self.highs.len() >= self.period
    }

    fn period(&self) -> usize {
        self.period
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
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let c = hp;
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: c, high: hp, low: lp, close: c,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    fn flat_bar(c: &str) -> OhlcvBar { bar(c, c) }

    #[test]
    fn test_bars_since_invalid() {
        assert!(BarsSince::new("b", 0).is_err());
    }

    #[test]
    fn test_bars_since_unavailable_before_period() {
        let mut bs = BarsSince::new("b", 3).unwrap();
        assert_eq!(bs.update_bar(&flat_bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(bs.update_bar(&flat_bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bars_since_flat_is_zero() {
        let mut bs = BarsSince::new("b", 3).unwrap();
        let mut last = SignalValue::Unavailable;
        for _ in 0..5 { last = bs.update_bar(&flat_bar("100")).unwrap(); }
        if let SignalValue::Scalar(v) = last {
            assert_eq!(v, dec!(0), "flat series: bsh == bsl == 0 → diff=0");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_bars_since_high_recent() {
        // period=3: bars [L=80, N=100, H=120] → high at index 2 (0 bars ago), low at index 0 (2 bars ago)
        // bsh=0, bsl=2 → scalar = 0 - 2 = -2 (high more recent → negative)
        let mut bs = BarsSince::new("b", 3).unwrap();
        bs.update_bar(&bar("80", "80")).unwrap();
        bs.update_bar(&bar("100", "100")).unwrap();
        if let SignalValue::Scalar(v) = bs.update_bar(&bar("120", "120")).unwrap() {
            assert!(v < dec!(0), "high set most recently → negative: {v}");
        } else { panic!("expected Scalar"); }
    }

    #[test]
    fn test_bars_since_reset() {
        let mut bs = BarsSince::new("b", 3).unwrap();
        for _ in 0..3 { bs.update_bar(&flat_bar("100")).unwrap(); }
        assert!(bs.is_ready());
        bs.reset();
        assert!(!bs.is_ready());
    }
}
