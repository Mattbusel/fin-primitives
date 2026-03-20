import os

base = "src/signals/indicators"

price_level_pct = """\
//! Price Level Percent indicator -- close position within rolling period high-low range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Level Percent -- close position within the rolling period's high-low range (0-100%).
///
/// ```text
/// period_high = max(high, period)
/// period_low  = min(low, period)
/// level[t]    = (close - period_low) / (period_high - period_low) * 100
/// ```
///
/// - 0%   → close at the lowest low of the period (bearish)
/// - 100% → close at the highest high of the period (bullish)
/// - 50%  → close at midpoint of the period range
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated
/// or if the period high equals the period low (flat market).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceLevelPct;
/// use fin_primitives::signals::Signal;
/// let pl = PriceLevelPct::new("pl", 20).unwrap();
/// assert_eq!(pl.period(), 20);
/// ```
pub struct PriceLevelPct {
    name: String,
    period: usize,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
}

impl PriceLevelPct {
    /// Constructs a new `PriceLevelPct`.
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

impl Signal for PriceLevelPct {
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
        let range = period_high - period_low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar((bar.close - period_low) / range * Decimal::ONE_HUNDRED))
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
    fn test_pl_period_0_error() { assert!(PriceLevelPct::new("pl", 0).is_err()); }

    #[test]
    fn test_pl_unavailable_before_period() {
        let mut pl = PriceLevelPct::new("pl", 3).unwrap();
        assert_eq!(pl.update_bar(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pl_close_at_top_is_100() {
        let mut pl = PriceLevelPct::new("pl", 3).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        // Period high=110, low=90; close=110 -> (110-90)/(110-90)*100 = 100
        let v = pl.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_pl_close_at_bottom_is_0() {
        let mut pl = PriceLevelPct::new("pl", 3).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        let v = pl.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pl_midpoint_is_50() {
        let mut pl = PriceLevelPct::new("pl", 2).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        let v = pl.update_bar(&bar("110", "90", "100")).unwrap();
        // period_high=110, period_low=90, close=100 -> (100-90)/20*100 = 50
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_pl_flat_market_unavailable() {
        let mut pl = PriceLevelPct::new("pl", 2).unwrap();
        pl.update_bar(&bar("100", "100", "100")).unwrap();
        let v = pl.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_pl_reset() {
        let mut pl = PriceLevelPct::new("pl", 2).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        pl.update_bar(&bar("110", "90", "100")).unwrap();
        assert!(pl.is_ready());
        pl.reset();
        assert!(!pl.is_ready());
    }
}
"""

volume_open_bias = """\
//! Volume Open Bias indicator -- fraction of rolling volume from gap-up opens.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Open Bias -- the fraction of total rolling volume occurring on bars that
/// open above the previous close (gap-up bars), scaled to 0-100%.
///
/// A high value indicates most volume flows into gap-up sessions (bullish bias).
/// A low value suggests volume predominantly occurs on flat or gap-down opens.
///
/// ```text
/// gap_vol[t] = volume[t] if open[t] > prev_close[t-1], else 0
/// bias[t]    = sum(gap_vol, period) / sum(volume, period) * 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars with a valid prior close
/// have been seen, or if total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeOpenBias;
/// use fin_primitives::signals::Signal;
/// let vob = VolumeOpenBias::new("vob", 10).unwrap();
/// assert_eq!(vob.period(), 10);
/// ```
pub struct VolumeOpenBias {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    gap_window: VecDeque<Decimal>,
    vol_window: VecDeque<Decimal>,
    gap_sum: Decimal,
    vol_sum: Decimal,
}

impl VolumeOpenBias {
    /// Constructs a new `VolumeOpenBias`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            gap_window: VecDeque::with_capacity(period),
            vol_window: VecDeque::with_capacity(period),
            gap_sum: Decimal::ZERO,
            vol_sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeOpenBias {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.gap_window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let gap_vol = match self.prev_close {
            Some(pc) if bar.open > pc => bar.volume,
            _ => Decimal::ZERO,
        };
        self.prev_close = Some(bar.close);

        self.gap_window.push_back(gap_vol);
        self.vol_window.push_back(bar.volume);
        self.gap_sum += gap_vol;
        self.vol_sum += bar.volume;
        if self.gap_window.len() > self.period {
            if let Some(old_g) = self.gap_window.pop_front() { self.gap_sum -= old_g; }
            if let Some(old_v) = self.vol_window.pop_front() { self.vol_sum -= old_v; }
        }
        if self.gap_window.len() < self.period { return Ok(SignalValue::Unavailable); }
        if self.vol_sum.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(self.gap_sum / self.vol_sum * Decimal::ONE_HUNDRED))
    }

    fn reset(&mut self) {
        self.prev_close = None;
        self.gap_window.clear();
        self.vol_window.clear();
        self.gap_sum = Decimal::ZERO;
        self.vol_sum = Decimal::ZERO;
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
    fn test_vob_period_0_error() { assert!(VolumeOpenBias::new("vob", 0).is_err()); }

    #[test]
    fn test_vob_unavailable_before_period() {
        let mut vob = VolumeOpenBias::new("vob", 3).unwrap();
        assert_eq!(vob.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vob_all_gap_up_is_100() {
        let mut vob = VolumeOpenBias::new("vob", 3).unwrap();
        // Bar 1: sets prev_close=102. No comparison yet, gap_vol=0.
        vob.update_bar(&bar("100", "102", "1000")).unwrap();
        // Bar 2: open=105 > prev_close=102 -> gap_vol=1000
        vob.update_bar(&bar("105", "107", "1000")).unwrap();
        // Bar 3: open=110 > prev_close=107 -> gap_vol=1000
        vob.update_bar(&bar("110", "112", "1000")).unwrap();
        // Bar 4 (window slides): open=115 > prev_close=112 -> gap_vol=1000
        // window=[1000,1000,1000], vol=[1000,1000,1000] -> 100%
        let v = vob.update_bar(&bar("115", "117", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_vob_no_gap_up_is_0() {
        let mut vob = VolumeOpenBias::new("vob", 3).unwrap();
        // Each bar opens at or below previous close -> no gap-up
        vob.update_bar(&bar("105", "100", "1000")).unwrap(); // prev_close=100
        vob.update_bar(&bar("100", "95", "1000")).unwrap();  // open=100, prev=100 -> no gap
        vob.update_bar(&bar("95",  "90", "1000")).unwrap();  // open=95, prev=95 -> no gap
        let v = vob.update_bar(&bar("90", "85", "1000")).unwrap();  // open=90, prev=90 -> no gap
        // window=[0,0,0], vol=[1000,1000,1000] -> 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vob_reset() {
        let mut vob = VolumeOpenBias::new("vob", 2).unwrap();
        vob.update_bar(&bar("100", "102", "1000")).unwrap();
        vob.update_bar(&bar("105", "107", "1000")).unwrap();
        vob.update_bar(&bar("110", "112", "1000")).unwrap();
        assert!(vob.is_ready());
        vob.reset();
        assert!(!vob.is_ready());
    }
}
"""

range_momentum = """\
//! Range Momentum indicator -- rate of change in bar range (volatility momentum).

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Momentum -- rate of change of the bar's range (high - low) over `period` bars.
///
/// ```text
/// range[t]   = high[t] - low[t]
/// momentum   = (range[t] - range[t - period]) / range[t - period] * 100
/// ```
///
/// Positive values indicate volatility expansion; negative values indicate contraction.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// or if the prior range is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeMomentum;
/// use fin_primitives::signals::Signal;
/// let rm = RangeMomentum::new("rm", 10).unwrap();
/// assert_eq!(rm.period(), 10);
/// ```
pub struct RangeMomentum {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
}

impl RangeMomentum {
    /// Constructs a new `RangeMomentum`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period + 1),
        })
    }
}

impl Signal for RangeMomentum {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() > self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.window.push_back(range);
        if self.window.len() > self.period + 1 {
            self.window.pop_front();
        }
        if self.window.len() <= self.period { return Ok(SignalValue::Unavailable); }
        let prior = self.window[0];
        if prior.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar((range - prior) / prior * Decimal::ONE_HUNDRED))
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let mp = Price::new((hp.value() + lp.value()) / Decimal::TWO).unwrap();
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
    fn test_rm_period_0_error() { assert!(RangeMomentum::new("rm", 0).is_err()); }

    #[test]
    fn test_rm_unavailable_before_period_plus_one() {
        let mut rm = RangeMomentum::new("rm", 3).unwrap();
        // period=3 needs 4 bars total; 3rd bar is still unavailable
        rm.update_bar(&bar("110", "90")).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        let v = rm.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rm_same_range_is_zero() {
        let mut rm = RangeMomentum::new("rm", 3).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        // 4th bar: range=20, prior(bar1)=20 -> 0%
        let v = rm.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rm_expansion_positive() {
        let mut rm = RangeMomentum::new("rm", 1).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();  // range=20
        // 2nd bar: range=40, prior=20 -> (40-20)/20*100 = 100%
        let v = rm.update_bar(&bar("120", "80")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_rm_contraction_negative() {
        let mut rm = RangeMomentum::new("rm", 1).unwrap();
        rm.update_bar(&bar("120", "80")).unwrap();  // range=40
        // 2nd bar: range=20, prior=40 -> (20-40)/40*100 = -50%
        let v = rm.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-50)));
    }

    #[test]
    fn test_rm_zero_prior_range_unavailable() {
        let mut rm = RangeMomentum::new("rm", 1).unwrap();
        rm.update_bar(&bar("100", "100")).unwrap(); // range=0
        let v = rm.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rm_reset() {
        let mut rm = RangeMomentum::new("rm", 2).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        rm.update_bar(&bar("110", "90")).unwrap();
        assert!(rm.is_ready());
        rm.reset();
        assert!(!rm.is_ready());
    }
}
"""

close_above_prev_close = """\
//! Close Above Prev Close indicator -- rolling % of bars where close > previous close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Above Prev Close -- rolling percentage of bars where close > previous bar close.
///
/// Measures bullish follow-through: a high value (near 100%) means the instrument
/// consistently closed higher than the prior bar over the lookback window.
///
/// ```text
/// above[t] = 1 if close[t] > close[t-1], else 0
/// ratio[t] = sum(above, period) / period x 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` comparisons have been made
/// (requires `period + 1` bars total).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAbovePrevClose;
/// use fin_primitives::signals::Signal;
/// let capc = CloseAbovePrevClose::new("capc", 10).unwrap();
/// assert_eq!(capc.period(), 10);
/// ```
pub struct CloseAbovePrevClose {
    name: String,
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl CloseAbovePrevClose {
    /// Constructs a new `CloseAbovePrevClose`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            prev_close: None,
            window: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for CloseAbovePrevClose {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let above: u8 = if bar.close > pc { 1 } else { 0 };
            self.window.push_back(above);
            self.count += above as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() { self.count -= old as usize; }
            }
        }
        self.prev_close = Some(bar.close);
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let ratio = Decimal::from(self.count as u32)
            / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
        self.prev_close = None;
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
    fn test_capc_period_0_error() { assert!(CloseAbovePrevClose::new("capc", 0).is_err()); }

    #[test]
    fn test_capc_unavailable_before_period() {
        let mut capc = CloseAbovePrevClose::new("capc", 3).unwrap();
        // bar1: no prev, window=[] -> Unavailable
        assert_eq!(capc.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        // bar2: prev=100, 101>100, window=[1] -> Unavailable (1 < 3)
        assert_eq!(capc.update_bar(&bar("101")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_capc_all_rising_is_100() {
        let mut capc = CloseAbovePrevClose::new("capc", 3).unwrap();
        capc.update_bar(&bar("100")).unwrap(); // no comparison yet
        capc.update_bar(&bar("101")).unwrap(); // 101>100 -> window=[1]
        capc.update_bar(&bar("102")).unwrap(); // 102>101 -> window=[1,1]
        let v = capc.update_bar(&bar("103")).unwrap(); // 103>102 -> window=[1,1,1] -> 100%
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_capc_all_falling_is_0() {
        let mut capc = CloseAbovePrevClose::new("capc", 3).unwrap();
        capc.update_bar(&bar("103")).unwrap();
        capc.update_bar(&bar("102")).unwrap(); // not above -> window=[0]
        capc.update_bar(&bar("101")).unwrap(); // not above -> window=[0,0]
        let v = capc.update_bar(&bar("100")).unwrap(); // not above -> window=[0,0,0] -> 0%
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_capc_window_slides() {
        let mut capc = CloseAbovePrevClose::new("capc", 2).unwrap();
        capc.update_bar(&bar("100")).unwrap(); // no comparison
        capc.update_bar(&bar("101")).unwrap(); // above -> window=[1]
        capc.update_bar(&bar("102")).unwrap(); // above -> window=[1,1] -> 100%
        let v = capc.update_bar(&bar("101")).unwrap(); // not above -> window=[1,0] -> 50%
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_capc_reset() {
        let mut capc = CloseAbovePrevClose::new("capc", 3).unwrap();
        for p in ["100", "101", "102", "103"] { capc.update_bar(&bar(p)).unwrap(); }
        assert!(capc.is_ready());
        capc.reset();
        assert!(!capc.is_ready());
    }
}
"""

files = {
    "price_level_pct": price_level_pct,
    "volume_open_bias": volume_open_bias,
    "range_momentum": range_momentum,
    "close_above_prev_close": close_above_prev_close,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
