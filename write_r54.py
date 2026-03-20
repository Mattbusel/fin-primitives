import os

base = "src/signals/indicators"

close_above_midpoint = """\
//! Close Above Midpoint indicator -- rolling % of bars where close > bar midpoint.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Close Above Midpoint -- percentage of bars in the last `period` where the close
/// is above the bar's own midpoint `(high + low) / 2`.
///
/// When close is above the midpoint, buyers dominated that bar. Values near 100%
/// indicate consistent bullish closes; near 0% indicates persistent bearish closes.
///
/// ```text
/// mid[t]       = (high + low) / 2
/// above[t]     = 1 if close[t] > mid[t], else 0
/// ratio[t]     = sum(above, period) / period x 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CloseAboveMidpoint;
/// use fin_primitives::signals::Signal;
/// let cam = CloseAboveMidpoint::new("cam", 10).unwrap();
/// assert_eq!(cam.period(), 10);
/// ```
pub struct CloseAboveMidpoint {
    name: String,
    period: usize,
    window: VecDeque<u8>,
    count: usize,
}

impl CloseAboveMidpoint {
    /// Constructs a new `CloseAboveMidpoint`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for CloseAboveMidpoint {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let mid = (bar.high + bar.low) / Decimal::TWO;
        let above: u8 = if bar.close > mid { 1 } else { 0 };
        self.window.push_back(above);
        self.count += above as usize;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.count -= old as usize; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let ratio = Decimal::from(self.count as u32)
            / Decimal::from(self.period as u32)
            * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(ratio))
    }

    fn reset(&mut self) {
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
    fn test_cam_period_0_error() { assert!(CloseAboveMidpoint::new("c", 0).is_err()); }

    #[test]
    fn test_cam_unavailable_before_period() {
        let mut c = CloseAboveMidpoint::new("c", 3).unwrap();
        assert_eq!(c.update_bar(&bar("110", "90", "108")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_cam_all_above_is_100() {
        let mut c = CloseAboveMidpoint::new("c", 3).unwrap();
        // mid = 100, close = 108 > 100 on all bars
        c.update_bar(&bar("110", "90", "108")).unwrap();
        c.update_bar(&bar("110", "90", "108")).unwrap();
        let v = c.update_bar(&bar("110", "90", "108")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_cam_all_below_is_0() {
        let mut c = CloseAboveMidpoint::new("c", 3).unwrap();
        // mid = 100, close = 92 < 100
        c.update_bar(&bar("110", "90", "92")).unwrap();
        c.update_bar(&bar("110", "90", "92")).unwrap();
        let v = c.update_bar(&bar("110", "90", "92")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cam_reset() {
        let mut c = CloseAboveMidpoint::new("c", 2).unwrap();
        c.update_bar(&bar("110", "90", "108")).unwrap();
        c.update_bar(&bar("110", "90", "108")).unwrap();
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
    }
}
"""

volume_up_down_ratio = """\
//! Volume Up/Down Ratio indicator -- rolling ratio of up-bar volume to total volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Up/Down Ratio -- the fraction of total volume that occurred on up-bars
/// (bars where close > open) over the last `period` bars.
///
/// ```text
/// up_vol[t]    = volume[t] if close > open, else 0
/// ratio[t]     = sum(up_vol, period) / sum(volume, period) x 100
/// ```
///
/// Values > 50 mean more volume occurred on up-bars (bullish accumulation);
/// < 50 indicates distribution/bearish pressure.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or
/// total volume is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeUpDownRatio;
/// use fin_primitives::signals::Signal;
/// let vudr = VolumeUpDownRatio::new("vudr", 10).unwrap();
/// assert_eq!(vudr.period(), 10);
/// ```
pub struct VolumeUpDownRatio {
    name: String,
    period: usize,
    up_window: VecDeque<Decimal>,
    total_window: VecDeque<Decimal>,
    up_sum: Decimal,
    total_sum: Decimal,
}

impl VolumeUpDownRatio {
    /// Constructs a new `VolumeUpDownRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            up_window: VecDeque::with_capacity(period),
            total_window: VecDeque::with_capacity(period),
            up_sum: Decimal::ZERO,
            total_sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeUpDownRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.total_window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let up_vol = if bar.close > bar.open { bar.volume } else { Decimal::ZERO };
        self.up_window.push_back(up_vol);
        self.total_window.push_back(bar.volume);
        self.up_sum += up_vol;
        self.total_sum += bar.volume;
        if self.total_window.len() > self.period {
            if let Some(old_u) = self.up_window.pop_front() { self.up_sum -= old_u; }
            if let Some(old_t) = self.total_window.pop_front() { self.total_sum -= old_t; }
        }
        if self.total_window.len() < self.period { return Ok(SignalValue::Unavailable); }
        if self.total_sum.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar(self.up_sum / self.total_sum * Decimal::ONE_HUNDRED))
    }

    fn reset(&mut self) {
        self.up_window.clear();
        self.total_window.clear();
        self.up_sum = Decimal::ZERO;
        self.total_sum = Decimal::ZERO;
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
    fn test_vudr_period_0_error() { assert!(VolumeUpDownRatio::new("v", 0).is_err()); }

    #[test]
    fn test_vudr_unavailable_before_period() {
        let mut v = VolumeUpDownRatio::new("v", 3).unwrap();
        assert_eq!(v.update_bar(&bar("100", "105", "1000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vudr_all_up_bars_is_100() {
        let mut v = VolumeUpDownRatio::new("v", 3).unwrap();
        // all up bars, equal volume
        v.update_bar(&bar("100", "105", "1000")).unwrap();
        v.update_bar(&bar("100", "105", "1000")).unwrap();
        let r = v.update_bar(&bar("100", "105", "1000")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_vudr_all_down_bars_is_0() {
        let mut v = VolumeUpDownRatio::new("v", 3).unwrap();
        v.update_bar(&bar("105", "100", "1000")).unwrap();
        v.update_bar(&bar("105", "100", "1000")).unwrap();
        let r = v.update_bar(&bar("105", "100", "1000")).unwrap();
        assert_eq!(r, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vudr_half_up_volume() {
        let mut v = VolumeUpDownRatio::new("v", 2).unwrap();
        v.update_bar(&bar("100", "105", "1000")).unwrap(); // up, 1000
        let r = v.update_bar(&bar("105", "100", "1000")).unwrap(); // down, 1000 -> up_sum=1000, total=2000 -> 50%
        assert_eq!(r, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_vudr_reset() {
        let mut v = VolumeUpDownRatio::new("v", 2).unwrap();
        v.update_bar(&bar("100", "105", "1000")).unwrap();
        v.update_bar(&bar("100", "105", "1000")).unwrap();
        assert!(v.is_ready());
        v.reset();
        assert!(!v.is_ready());
    }
}
"""

files = {
    "close_above_midpoint": close_above_midpoint,
    "volume_up_down_ratio": volume_up_down_ratio,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
