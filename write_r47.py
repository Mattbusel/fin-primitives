import os, sys

base = "src/signals/indicators"

price_to_sma_ratio = """\
//! Price-to-SMA Ratio indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price-to-SMA Ratio -- how far price has extended above or below its N-period SMA.
///
/// ```text
/// sma[t]   = SMA(close, period)
/// ratio[t] = close[t] / sma[t]
/// ```
///
/// A ratio above 1.0 means price is above its moving average; below 1.0 means below.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have accumulated or if SMA is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceToSmaRatio;
/// use fin_primitives::signals::Signal;
/// let p = PriceToSmaRatio::new("ptsr", 20).unwrap();
/// assert_eq!(p.period(), 20);
/// ```
pub struct PriceToSmaRatio {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceToSmaRatio {
    /// Constructs a new `PriceToSmaRatio`.
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

impl Signal for PriceToSmaRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        self.sum += bar.close;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let sma = self.sum / Decimal::from(self.period as u32);
        if sma.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(bar.close / sma))
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

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ptsr_period_0_error() { assert!(PriceToSmaRatio::new("p", 0).is_err()); }

    #[test]
    fn test_ptsr_unavailable_before_period() {
        let mut p = PriceToSmaRatio::new("p", 3).unwrap();
        assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ptsr_at_sma_is_one() {
        let mut p = PriceToSmaRatio::new("p", 3).unwrap();
        // constant price => close == SMA => ratio = 1
        p.update_bar(&bar("100")).unwrap();
        p.update_bar(&bar("100")).unwrap();
        let v = p.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_ptsr_above_sma() {
        // SMA(10,10,20) = 40/3, close=20 => ratio = 20/(40/3) = 1.5
        let mut p = PriceToSmaRatio::new("p", 3).unwrap();
        p.update_bar(&bar("10")).unwrap();
        p.update_bar(&bar("10")).unwrap();
        let v = p.update_bar(&bar("20")).unwrap();
        if let SignalValue::Scalar(ratio) = v {
            assert!(ratio > dec!(1), "expected ratio > 1, got {ratio}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ptsr_reset() {
        let mut p = PriceToSmaRatio::new("p", 2).unwrap();
        p.update_bar(&bar("100")).unwrap();
        p.update_bar(&bar("100")).unwrap();
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
"""

high_of_period = """\
//! High of Period indicator -- rolling N-bar highest high.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// High of Period -- the highest high seen over the last `period` bars.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::HighOfPeriod;
/// use fin_primitives::signals::Signal;
/// let h = HighOfPeriod::new("hop", 20).unwrap();
/// assert_eq!(h.period(), 20);
/// ```
pub struct HighOfPeriod {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl HighOfPeriod {
    /// Constructs a new `HighOfPeriod`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self { name: name.into(), period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for HighOfPeriod {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.high);
        if self.window.len() > self.period { self.window.pop_front(); }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        let max = self.window.iter().copied().fold(Decimal::MIN, Decimal::max);
        Ok(SignalValue::Scalar(max))
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str) -> OhlcvBar {
        let p = Price::new(h.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_hop_period_0_error() { assert!(HighOfPeriod::new("h", 0).is_err()); }

    #[test]
    fn test_hop_unavailable_before_period() {
        let mut h = HighOfPeriod::new("h", 3).unwrap();
        assert_eq!(h.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_hop_returns_max() {
        let mut h = HighOfPeriod::new("h", 3).unwrap();
        h.update_bar(&bar("90")).unwrap();
        h.update_bar(&bar("110")).unwrap();
        let v = h.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(110)));
    }

    #[test]
    fn test_hop_rolls_out_old_max() {
        let mut h = HighOfPeriod::new("h", 3).unwrap();
        h.update_bar(&bar("150")).unwrap(); // will roll out
        h.update_bar(&bar("90")).unwrap();
        h.update_bar(&bar("95")).unwrap(); // window full, max=150
        let v = h.update_bar(&bar("100")).unwrap(); // 150 rolls out, max=100
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_hop_reset() {
        let mut h = HighOfPeriod::new("h", 2).unwrap();
        h.update_bar(&bar("100")).unwrap();
        h.update_bar(&bar("110")).unwrap();
        assert!(h.is_ready());
        h.reset();
        assert!(!h.is_ready());
    }
}
"""

low_of_period = """\
//! Low of Period indicator -- rolling N-bar lowest low.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Low of Period -- the lowest low seen over the last `period` bars.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::LowOfPeriod;
/// use fin_primitives::signals::Signal;
/// let l = LowOfPeriod::new("lop", 20).unwrap();
/// assert_eq!(l.period(), 20);
/// ```
pub struct LowOfPeriod {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl LowOfPeriod {
    /// Constructs a new `LowOfPeriod`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self { name: name.into(), period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for LowOfPeriod {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.low);
        if self.window.len() > self.period { self.window.pop_front(); }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        let min = self.window.iter().copied().fold(Decimal::MAX, Decimal::min);
        Ok(SignalValue::Scalar(min))
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(l: &str) -> OhlcvBar {
        let p = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_lop_period_0_error() { assert!(LowOfPeriod::new("l", 0).is_err()); }

    #[test]
    fn test_lop_unavailable_before_period() {
        let mut l = LowOfPeriod::new("l", 3).unwrap();
        assert_eq!(l.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_lop_returns_min() {
        let mut l = LowOfPeriod::new("l", 3).unwrap();
        l.update_bar(&bar("90")).unwrap();
        l.update_bar(&bar("110")).unwrap();
        let v = l.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(90)));
    }

    #[test]
    fn test_lop_rolls_out_old_min() {
        let mut l = LowOfPeriod::new("l", 3).unwrap();
        l.update_bar(&bar("50")).unwrap(); // will roll out
        l.update_bar(&bar("90")).unwrap();
        l.update_bar(&bar("95")).unwrap(); // window full, min=50
        let v = l.update_bar(&bar("80")).unwrap(); // 50 rolls out, min=80
        assert_eq!(v, SignalValue::Scalar(dec!(80)));
    }

    #[test]
    fn test_lop_reset() {
        let mut l = LowOfPeriod::new("l", 2).unwrap();
        l.update_bar(&bar("100")).unwrap();
        l.update_bar(&bar("90")).unwrap();
        assert!(l.is_ready());
        l.reset();
        assert!(!l.is_ready());
    }
}
"""

bar_close_rank = """\
//! Bar Close Rank indicator -- percentile rank of today's close within last N closes.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Bar Close Rank -- percentile rank (0-100) of the current close within the last `period` bars.
///
/// A rank of 100 means today's close is the highest close in the window.
/// A rank of 0 means it is the lowest.
///
/// ```text
/// rank[t] = (count of past closes strictly less than close[t]) / (period - 1) x 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
/// When `period == 1`, always returns 50 (single-element rank is undefined; midpoint is used).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::BarCloseRank;
/// use fin_primitives::signals::Signal;
/// let bcr = BarCloseRank::new("bcr", 10).unwrap();
/// assert_eq!(bcr.period(), 10);
/// ```
pub struct BarCloseRank {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl BarCloseRank {
    /// Constructs a new `BarCloseRank`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self { name: name.into(), period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for BarCloseRank {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.close);
        if self.window.len() > self.period { self.window.pop_front(); }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }

        if self.period == 1 {
            return Ok(SignalValue::Scalar(Decimal::from(50u32)));
        }

        let current = bar.close;
        let below = self.window.iter().filter(|&&v| v < current).count();
        #[allow(clippy::cast_possible_truncation)]
        let rank = Decimal::from(below as u32)
            / Decimal::from((self.period - 1) as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(rank))
    }

    fn reset(&mut self) {
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> OhlcvBar {
        let p = Price::new(c.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_bcr_period_0_error() { assert!(BarCloseRank::new("b", 0).is_err()); }

    #[test]
    fn test_bcr_unavailable_before_period() {
        let mut b = BarCloseRank::new("b", 3).unwrap();
        assert_eq!(b.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_bcr_highest_close_is_100() {
        // window [90, 95, 100] -- current 100 is highest
        let mut b = BarCloseRank::new("b", 3).unwrap();
        b.update_bar(&bar("90")).unwrap();
        b.update_bar(&bar("95")).unwrap();
        let v = b.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_bcr_lowest_close_is_0() {
        // window [90, 95, 100] -- if current bar is 80 (lowest), rank=0
        let mut b = BarCloseRank::new("b", 3).unwrap();
        b.update_bar(&bar("90")).unwrap();
        b.update_bar(&bar("95")).unwrap();
        // roll in 80, roll out 90 -- window=[95, 100, 80] -- wait, period=3
        // actually: window=[90,95] then push 80 -> [90,95,80], 80<90 and 80<95, below=0
        let v = b.update_bar(&bar("80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_bcr_midpoint() {
        // window [90, 100, 95] (period=3), current=95, below=[90]=1, denom=2 => 50
        let mut b = BarCloseRank::new("b", 3).unwrap();
        b.update_bar(&bar("90")).unwrap();
        b.update_bar(&bar("100")).unwrap();
        let v = b.update_bar(&bar("95")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_bcr_reset() {
        let mut b = BarCloseRank::new("b", 2).unwrap();
        b.update_bar(&bar("90")).unwrap();
        b.update_bar(&bar("100")).unwrap();
        assert!(b.is_ready());
        b.reset();
        assert!(!b.is_ready());
    }
}
"""

files = {
    "price_to_sma_ratio": price_to_sma_ratio,
    "high_of_period": high_of_period,
    "low_of_period": low_of_period,
    "bar_close_rank": bar_close_rank,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    # Write as UTF-8 with LF line endings
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
