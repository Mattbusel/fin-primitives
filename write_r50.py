import os

base = "src/signals/indicators"

price_zscore = """\
//! Price Z-Score indicator -- rolling z-score of close price.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Z-Score -- how many standard deviations the current close is from its
/// rolling N-period mean.
///
/// ```text
/// mean[t]    = SMA(close, period)
/// stddev[t]  = sample stddev of close over period
/// zscore[t]  = (close[t] - mean[t]) / stddev[t]
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or when
/// standard deviation is zero (all prices identical).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceZScore;
/// use fin_primitives::signals::Signal;
/// let pz = PriceZScore::new("pz", 20).unwrap();
/// assert_eq!(pz.period(), 20);
/// ```
pub struct PriceZScore {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl PriceZScore {
    /// Constructs a new `PriceZScore`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period < 2` (need at least 2 values for stddev).
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period < 2 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceZScore {
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
        let n = Decimal::from(self.period as u32);
        let mean = self.sum / n;

        // Sample variance
        let variance = self.window.iter()
            .map(|v| {
                let diff = *v - mean;
                diff * diff
            })
            .fold(Decimal::ZERO, |acc, v| acc + v)
            / (n - Decimal::ONE);

        if variance <= Decimal::ZERO {
            return Ok(SignalValue::Unavailable);
        }

        // sqrt via Newton-Raphson on Decimal
        let variance_f: f64 = variance.to_string().parse().unwrap_or(f64::NAN);
        if variance_f.is_nan() { return Ok(SignalValue::Unavailable); }
        let stddev_f = variance_f.sqrt();
        let stddev = match Decimal::try_from(stddev_f) {
            Ok(d) if !d.is_zero() => d,
            _ => return Ok(SignalValue::Unavailable),
        };

        Ok(SignalValue::Scalar((bar.close - mean) / stddev))
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
    fn test_pz_period_less_than_2_error() { assert!(PriceZScore::new("p", 1).is_err()); }
    #[test]
    fn test_pz_period_0_error() { assert!(PriceZScore::new("p", 0).is_err()); }

    #[test]
    fn test_pz_unavailable_before_period() {
        let mut p = PriceZScore::new("p", 3).unwrap();
        assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pz_constant_price_unavailable() {
        // stddev = 0 for constant prices
        let mut p = PriceZScore::new("p", 3).unwrap();
        for _ in 0..3 { p.update_bar(&bar("100")).unwrap(); }
        assert_eq!(p.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_pz_mean_price_is_zero() {
        // close == mean -> z-score = 0
        let mut p = PriceZScore::new("p", 3).unwrap();
        p.update_bar(&bar("90")).unwrap();
        p.update_bar(&bar("110")).unwrap();
        // Third bar at 100 (mean of 90,110,100 = 100). z-score(100) should be 0.
        let v = p.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(z) = v {
            assert!(z.abs() < dec!(0.001), "expected ~0 z-score at mean, got {z}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_pz_reset() {
        let mut p = PriceZScore::new("p", 3).unwrap();
        for _ in 0..3 { p.update_bar(&bar("100")).unwrap(); }
        assert!(p.is_ready());
        p.reset();
        assert!(!p.is_ready());
    }
}
"""

up_bar_ratio = """\
//! Up Bar Ratio indicator -- fraction of up-bars (close > open) over last N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Up Bar Ratio -- percentage of bars where close > open over a rolling `period`-bar window.
///
/// Similar to [`crate::signals::indicators::CloseAboveOpen`] but with a clearer name
/// emphasizing it uses the open-to-close comparison within each bar.
///
/// ```text
/// up_bar[t]     = 1 if close > open, else 0
/// ratio[t]      = sum(up_bar, period) / period x 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::UpBarRatio;
/// use fin_primitives::signals::Signal;
/// let ubr = UpBarRatio::new("ubr", 10).unwrap();
/// assert_eq!(ubr.period(), 10);
/// ```
pub struct UpBarRatio {
    name: String,
    period: usize,
    window: VecDeque<u8>,
    count: usize,
}

impl UpBarRatio {
    /// Constructs a new `UpBarRatio`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self { name: name.into(), period, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for UpBarRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let up: u8 = if bar.close > bar.open { 1 } else { 0 };
        self.window.push_back(up);
        self.count += up as usize;
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

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let high = if cp.value() > op.value() { cp } else { op };
        let low  = if cp.value() < op.value() { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high, low, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ubr_period_0_error() { assert!(UpBarRatio::new("u", 0).is_err()); }

    #[test]
    fn test_ubr_unavailable_before_period() {
        let mut u = UpBarRatio::new("u", 3).unwrap();
        assert_eq!(u.update_bar(&bar("100", "105")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_ubr_all_up_is_100() {
        let mut u = UpBarRatio::new("u", 3).unwrap();
        for _ in 0..3 { u.update_bar(&bar("100", "105")).unwrap(); }
        let v = u.update_bar(&bar("100", "105")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_ubr_all_down_is_0() {
        let mut u = UpBarRatio::new("u", 3).unwrap();
        u.update_bar(&bar("105", "100")).unwrap();
        u.update_bar(&bar("105", "100")).unwrap();
        let v = u.update_bar(&bar("105", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ubr_half_half_is_50() {
        let mut u = UpBarRatio::new("u", 4).unwrap();
        u.update_bar(&bar("100", "105")).unwrap();
        u.update_bar(&bar("105", "100")).unwrap();
        u.update_bar(&bar("100", "105")).unwrap();
        let v = u.update_bar(&bar("105", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_ubr_reset() {
        let mut u = UpBarRatio::new("u", 2).unwrap();
        u.update_bar(&bar("100", "105")).unwrap();
        u.update_bar(&bar("100", "105")).unwrap();
        assert!(u.is_ready());
        u.reset();
        assert!(!u.is_ready());
    }
}
"""

range_expansion_index = """\
//! Range Expansion Index indicator -- current range vs its rolling average.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Range Expansion Index -- how much the current bar's range deviates from its
/// N-period average, expressed as a percentage.
///
/// ```text
/// range[t]   = high[t] - low[t]
/// avg[t]     = SMA(range, period)
/// rei[t]     = (range[t] - avg[t]) / avg[t] x 100
/// ```
///
/// Positive values indicate the current bar has a wider-than-average range (range expansion).
/// Negative values indicate range contraction.
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen or if avg is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RangeExpansionIndex;
/// use fin_primitives::signals::Signal;
/// let rei = RangeExpansionIndex::new("rei", 14).unwrap();
/// assert_eq!(rei.period(), 14);
/// ```
pub struct RangeExpansionIndex {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl RangeExpansionIndex {
    /// Constructs a new `RangeExpansionIndex`.
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

impl Signal for RangeExpansionIndex {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        self.window.push_back(range);
        self.sum += range;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let avg = self.sum / Decimal::from(self.period as u32);
        if avg.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar((range - avg) / avg * Decimal::ONE_HUNDRED))
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

    fn bar(h: &str, l: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: lp, high: hp, low: lp, close: hp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_rei_period_0_error() { assert!(RangeExpansionIndex::new("r", 0).is_err()); }

    #[test]
    fn test_rei_unavailable_before_period() {
        let mut r = RangeExpansionIndex::new("r", 3).unwrap();
        assert_eq!(r.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_rei_constant_range_is_zero() {
        // range always 20 -> REI = (20 - 20) / 20 * 100 = 0
        let mut r = RangeExpansionIndex::new("r", 3).unwrap();
        r.update_bar(&bar("110", "90")).unwrap();
        r.update_bar(&bar("110", "90")).unwrap();
        let v = r.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rei_expansion_positive() {
        let mut r = RangeExpansionIndex::new("r", 3).unwrap();
        // small ranges: 10 each
        r.update_bar(&bar("110", "100")).unwrap();
        r.update_bar(&bar("110", "100")).unwrap();
        // large spike: range=40 -> avg=(10+10+40)/3, rei = (40-avg)/avg*100 > 0
        let v = r.update_bar(&bar("140", "100")).unwrap();
        if let SignalValue::Scalar(rei) = v {
            assert!(rei > dec!(0), "expected positive REI for range expansion, got {rei}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_rei_reset() {
        let mut r = RangeExpansionIndex::new("r", 2).unwrap();
        r.update_bar(&bar("110", "90")).unwrap();
        r.update_bar(&bar("110", "90")).unwrap();
        assert!(r.is_ready());
        r.reset();
        assert!(!r.is_ready());
    }
}
"""

overnight_return = """\
//! Overnight Return indicator -- return from previous close to current open.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Overnight Return -- the percentage return from the previous bar's close to the
/// current bar's open.
///
/// ```text
/// overnight_return[t] = (open[t] - close[t-1]) / close[t-1] x 100
/// ```
///
/// Distinct from [`crate::signals::indicators::OpenGapPct`] only in name convention;
/// both compute the same overnight gap percentage. This variant is provided for
/// discoverability under the "return" naming convention.
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior close) or if
/// the prior close is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OvernightReturn;
/// use fin_primitives::signals::Signal;
/// let ovr = OvernightReturn::new("ovr");
/// assert_eq!(ovr.period(), 1);
/// ```
pub struct OvernightReturn {
    name: String,
    prev_close: Option<Decimal>,
}

impl OvernightReturn {
    /// Constructs a new `OvernightReturn`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_close: None }
    }
}

impl Signal for OvernightReturn {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) if pc.is_zero() => SignalValue::Unavailable,
            Some(pc) => SignalValue::Scalar((bar.open - pc) / pc * Decimal::ONE_HUNDRED),
        };
        self.prev_close = Some(bar.close);
        Ok(result)
    }

    fn reset(&mut self) {
        self.prev_close = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohlcv::OhlcvBar;
    use crate::signals::Signal;
    use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
    use rust_decimal_macros::dec;

    fn bar(o: &str, c: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let cp = Price::new(c.parse().unwrap()).unwrap();
        let high = if cp.value() > op.value() { cp } else { op };
        let low  = if cp.value() < op.value() { cp } else { op };
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high, low, close: cp,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ovr_first_bar_unavailable() {
        let mut ovr = OvernightReturn::new("ovr");
        assert_eq!(ovr.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
        assert!(ovr.is_ready());
    }

    #[test]
    fn test_ovr_gap_up() {
        let mut ovr = OvernightReturn::new("ovr");
        ovr.update_bar(&bar("100", "100")).unwrap(); // close=100
        let v = ovr.update_bar(&bar("110", "112")).unwrap(); // open=110, prev_close=100 -> +10%
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_ovr_gap_down() {
        let mut ovr = OvernightReturn::new("ovr");
        ovr.update_bar(&bar("100", "100")).unwrap();
        let v = ovr.update_bar(&bar("90", "88")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_ovr_no_gap() {
        let mut ovr = OvernightReturn::new("ovr");
        ovr.update_bar(&bar("100", "100")).unwrap();
        let v = ovr.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ovr_reset() {
        let mut ovr = OvernightReturn::new("ovr");
        ovr.update_bar(&bar("100", "102")).unwrap();
        assert!(ovr.is_ready());
        ovr.reset();
        assert!(!ovr.is_ready());
        assert_eq!(ovr.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
    }
}
"""

files = {
    "price_zscore": price_zscore,
    "up_bar_ratio": up_bar_ratio,
    "range_expansion_index": range_expansion_index,
    "overnight_return": overnight_return,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
