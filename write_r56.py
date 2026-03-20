import os

base = "src/signals/indicators"

cumulative_delta = """\
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
"""

close_retrace_pct = """\
//! Close Retrace Percent indicator -- how far close retraced from the bar high.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Close Retrace Percent -- measures how far the close has retraced from the bar's high
/// within the bar's total range.
///
/// ```text
/// retrace[t] = (high - close) / (high - low) x 100
/// ```
///
/// Interpretation:
/// - 0%   → close == high (fully bullish bar)
/// - 50%  → close at midpoint of range
/// - 100% → close == low (fully bearish bar)
///
/// Returns [`SignalValue::Unavailable`] if `high == low` (zero-range doji).
/// Becomes ready after the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseRetracePct;
/// use fin_primitives::signals::Signal;
/// let crp = CloseRetracePct::new("crp");
/// assert_eq!(crp.period(), 1);
/// ```
pub struct CloseRetracePct {
    name: String,
    ready: bool,
}

impl CloseRetracePct {
    /// Constructs a new `CloseRetracePct`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ready: false }
    }
}

impl Signal for CloseRetracePct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.ready }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.ready = true;
        let range = bar.high - bar.low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        let retrace = (bar.high - bar.close) / range * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(retrace))
    }

    fn reset(&mut self) {
        self.ready = false;
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
    fn test_crp_close_at_high_is_zero() {
        let mut crp = CloseRetracePct::new("crp");
        let v = crp.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_crp_close_at_low_is_100() {
        let mut crp = CloseRetracePct::new("crp");
        let v = crp.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_crp_close_at_midpoint_is_50() {
        let mut crp = CloseRetracePct::new("crp");
        let v = crp.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_crp_zero_range_unavailable() {
        let mut crp = CloseRetracePct::new("crp");
        let v = crp.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_crp_ready_after_first_bar() {
        let mut crp = CloseRetracePct::new("crp");
        assert!(!crp.is_ready());
        crp.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(crp.is_ready());
    }

    #[test]
    fn test_crp_reset() {
        let mut crp = CloseRetracePct::new("crp");
        crp.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(crp.is_ready());
        crp.reset();
        assert!(!crp.is_ready());
    }
}
"""

median_volume = """\
//! Median Volume indicator -- rolling median of bar volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Median Volume -- rolling median of bar volume over the last `period` bars.
///
/// Unlike the simple average, the median is robust to volume spikes. Useful for
/// detecting anomalous bars whose volume deviates significantly from the typical level.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::MedianVolume;
/// use fin_primitives::signals::Signal;
/// let mv = MedianVolume::new("mv", 20).unwrap();
/// assert_eq!(mv.period(), 20);
/// ```
pub struct MedianVolume {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl MedianVolume {
    /// Constructs a new `MedianVolume`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
        })
    }
}

impl Signal for MedianVolume {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        let mut sorted: Vec<Decimal> = self.window.iter().copied().collect();
        sorted.sort();
        let mid = sorted.len() / 2;
        let median = if sorted.len() % 2 == 1 {
            sorted[mid]
        } else {
            (sorted[mid - 1] + sorted[mid]) / Decimal::TWO
        };
        Ok(SignalValue::Scalar(median))
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

    fn bar(vol: &str) -> OhlcvBar {
        let p = Price::new(dec!(100)).unwrap();
        let v = Quantity::new(vol.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: p, high: p, low: p, close: p, volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_mv_period_0_error() { assert!(MedianVolume::new("mv", 0).is_err()); }

    #[test]
    fn test_mv_unavailable_before_period() {
        let mut mv = MedianVolume::new("mv", 3).unwrap();
        assert_eq!(mv.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_mv_odd_period_median() {
        let mut mv = MedianVolume::new("mv", 5).unwrap();
        for v in ["100", "200", "300", "400", "500"] { mv.update_bar(&bar(v)).unwrap(); }
        // window now full [100,200,300,400,500]; push 10000 -> slides to [200,300,400,500,10000]
        let r = mv.update_bar(&bar("10000")).unwrap();
        // sorted: [200,300,400,500,10000]; median = 400
        assert_eq!(r, SignalValue::Scalar(dec!(400)));
    }

    #[test]
    fn test_mv_even_period_median() {
        let mut mv = MedianVolume::new("mv", 4).unwrap();
        mv.update_bar(&bar("100")).unwrap();
        mv.update_bar(&bar("200")).unwrap();
        mv.update_bar(&bar("300")).unwrap();
        let r = mv.update_bar(&bar("400")).unwrap();
        // sorted: [100,200,300,400]; median = (200+300)/2 = 250
        assert_eq!(r, SignalValue::Scalar(dec!(250)));
    }

    #[test]
    fn test_mv_spike_resistant() {
        // Median should be near the typical value, not pulled toward spike
        let mut mv = MedianVolume::new("mv", 5).unwrap();
        mv.update_bar(&bar("100")).unwrap();
        mv.update_bar(&bar("100")).unwrap();
        mv.update_bar(&bar("100")).unwrap();
        mv.update_bar(&bar("100")).unwrap();
        let r = mv.update_bar(&bar("100000")).unwrap();
        // sorted: [100,100,100,100,100000]; median = 100
        assert_eq!(r, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_mv_reset() {
        let mut mv = MedianVolume::new("mv", 3).unwrap();
        for v in ["100", "200", "300"] { mv.update_bar(&bar(v)).unwrap(); }
        assert!(mv.is_ready());
        mv.reset();
        assert!(!mv.is_ready());
    }
}
"""

outside_bar_count = """\
//! Outside Bar Count indicator -- rolling count of outside bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Outside Bar Count -- rolling count of outside bars over the last `period` comparisons.
///
/// An outside bar has `high > prev_high AND low < prev_low`, meaning it completely
/// engulfs the previous bar's range. Such bars often signal volatility expansion
/// or indecision before a significant directional move.
///
/// ```text
/// outside[t] = 1 if high[t] > high[t-1] AND low[t] < low[t-1], else 0
/// count[t]   = sum(outside, period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated
/// (the first bar has no prior bar to compare against, so contributes 0).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OutsideBarCount;
/// use fin_primitives::signals::Signal;
/// let obc = OutsideBarCount::new("obc", 10).unwrap();
/// assert_eq!(obc.period(), 10);
/// ```
pub struct OutsideBarCount {
    name: String,
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl OutsideBarCount {
    /// Constructs a new `OutsideBarCount`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_high: None,
            prev_low: None,
            window: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for OutsideBarCount {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let outside: u8 = match (self.prev_high, self.prev_low) {
            (Some(ph), Some(pl)) if bar.high > ph && bar.low < pl => 1,
            _ => 0,
        };
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        self.window.push_back(outside);
        self.count += outside as usize;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.count -= old as usize; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn reset(&mut self) {
        self.prev_high = None;
        self.prev_low = None;
        self.window.clear();
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let mid_v = (hp.value() + lp.value()) / Decimal::TWO;
        let mp = Price::new(mid_v).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: mp, high: hp, low: lp, close: mp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_obc_period_0_error() { assert!(OutsideBarCount::new("obc", 0).is_err()); }

    #[test]
    fn test_obc_unavailable_before_period() {
        let mut obc = OutsideBarCount::new("obc", 3).unwrap();
        assert_eq!(obc.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_obc_no_outside_bars() {
        let mut obc = OutsideBarCount::new("obc", 3).unwrap();
        // identical bars: each bar's high == prev high → not outside
        obc.update_bar(&bar("110", "90")).unwrap();
        obc.update_bar(&bar("110", "90")).unwrap();
        obc.update_bar(&bar("110", "90")).unwrap();
        let v = obc.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_obc_all_outside_bars() {
        let mut obc = OutsideBarCount::new("obc", 3).unwrap();
        obc.update_bar(&bar("105", "95")).unwrap(); // first bar (no prev)
        obc.update_bar(&bar("110", "90")).unwrap(); // outside: 110>105, 90<95
        obc.update_bar(&bar("115", "85")).unwrap(); // outside: 115>110, 85<90
        let v = obc.update_bar(&bar("120", "80")).unwrap(); // outside: 120>115, 80<85
        // window=[1,1,1], count=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_obc_one_outside_in_window() {
        let mut obc = OutsideBarCount::new("obc", 3).unwrap();
        obc.update_bar(&bar("110", "90")).unwrap(); // first bar
        obc.update_bar(&bar("115", "85")).unwrap(); // outside ✓
        obc.update_bar(&bar("112", "88")).unwrap(); // not outside (112 < 115)
        let v = obc.update_bar(&bar("111", "89")).unwrap(); // not outside (111 < 112)
        // window after slide: [1, 0, 0], count=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_obc_reset() {
        let mut obc = OutsideBarCount::new("obc", 2).unwrap();
        obc.update_bar(&bar("110", "90")).unwrap();
        obc.update_bar(&bar("115", "85")).unwrap();
        obc.update_bar(&bar("120", "80")).unwrap();
        assert!(obc.is_ready());
        obc.reset();
        assert!(!obc.is_ready());
    }
}
"""

files = {
    "cumulative_delta": cumulative_delta,
    "close_retrace_pct": close_retrace_pct,
    "median_volume": median_volume,
    "outside_bar_count": outside_bar_count,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
