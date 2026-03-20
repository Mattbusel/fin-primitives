import os

base = "src/signals/indicators"

open_gap_pct = """\
//! Open Gap Percent indicator — overnight gap as a percentage of the prior close.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Open Gap Percent — measures the overnight gap as a percentage of the previous bar's close.
///
/// ```text
/// gap_pct[t] = (open[t] - close[t-1]) / close[t-1] × 100
/// ```
///
/// Positive values indicate a gap-up; negative values indicate a gap-down.
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior close).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenGapPct;
/// use fin_primitives::signals::Signal;
/// let ogp = OpenGapPct::new("ogp");
/// assert_eq!(ogp.period(), 1);
/// ```
pub struct OpenGapPct {
    name: String,
    prev_close: Option<Decimal>,
}

impl OpenGapPct {
    /// Constructs a new `OpenGapPct`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_close: None }
    }
}

impl Signal for OpenGapPct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) => {
                if pc.is_zero() {
                    SignalValue::Unavailable
                } else {
                    let gap = (bar.open - pc) / pc * Decimal::ONE_HUNDRED;
                    SignalValue::Scalar(gap)
                }
            }
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
    fn test_ogp_first_bar_unavailable() {
        let mut ogp = OpenGapPct::new("ogp");
        assert_eq!(ogp.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
        assert!(ogp.is_ready());
    }

    #[test]
    fn test_ogp_gap_up() {
        let mut ogp = OpenGapPct::new("ogp");
        ogp.update_bar(&bar("100", "100")).unwrap();
        let v = ogp.update_bar(&bar("110", "112")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_ogp_gap_down() {
        let mut ogp = OpenGapPct::new("ogp");
        ogp.update_bar(&bar("100", "100")).unwrap();
        let v = ogp.update_bar(&bar("90", "88")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_ogp_no_gap() {
        let mut ogp = OpenGapPct::new("ogp");
        ogp.update_bar(&bar("100", "100")).unwrap();
        let v = ogp.update_bar(&bar("100", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ogp_reset() {
        let mut ogp = OpenGapPct::new("ogp");
        ogp.update_bar(&bar("100", "102")).unwrap();
        assert!(ogp.is_ready());
        ogp.reset();
        assert!(!ogp.is_ready());
        assert_eq!(ogp.update_bar(&bar("100", "102")).unwrap(), SignalValue::Unavailable);
    }
}
"""

candle_range_ma = """\
//! Candle Range MA — SMA of bar range (high - low) over N bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Candle Range MA — simple moving average of bar range over `period` bars.
///
/// ```text
/// range[t]    = high[t] - low[t]
/// range_ma[t] = SMA(range, period)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CandleRangeMa;
/// use fin_primitives::signals::Signal;
/// let crm = CandleRangeMa::new("crm", 10).unwrap();
/// assert_eq!(crm.period(), 10);
/// ```
pub struct CandleRangeMa {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CandleRangeMa {
    /// Constructs a new `CandleRangeMa`.
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

impl Signal for CandleRangeMa {
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
        Ok(SignalValue::Scalar(avg))
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
    fn test_crm_period_0_error() { assert!(CandleRangeMa::new("crm", 0).is_err()); }

    #[test]
    fn test_crm_unavailable_before_period() {
        let mut crm = CandleRangeMa::new("crm", 3).unwrap();
        assert_eq!(crm.update_bar(&bar("110", "90")).unwrap(), SignalValue::Unavailable);
        assert!(!crm.is_ready());
    }

    #[test]
    fn test_crm_constant_range() {
        let mut crm = CandleRangeMa::new("crm", 3).unwrap();
        crm.update_bar(&bar("110", "90")).unwrap();
        crm.update_bar(&bar("110", "90")).unwrap();
        let v = crm.update_bar(&bar("110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(20)));
    }

    #[test]
    fn test_crm_rolling_average() {
        let mut crm = CandleRangeMa::new("crm", 2).unwrap();
        crm.update_bar(&bar("110", "100")).unwrap(); // range=10
        let v = crm.update_bar(&bar("120", "100")).unwrap(); // range=20, avg=15
        assert_eq!(v, SignalValue::Scalar(dec!(15)));
    }

    #[test]
    fn test_crm_reset() {
        let mut crm = CandleRangeMa::new("crm", 2).unwrap();
        crm.update_bar(&bar("110", "90")).unwrap();
        crm.update_bar(&bar("110", "90")).unwrap();
        assert!(crm.is_ready());
        crm.reset();
        assert!(!crm.is_ready());
    }
}
"""

relative_close = """\
//! Relative Close indicator — close position within the bar's range, as a percentage.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Relative Close — where the close falls within the bar's high-low range, as a percentage.
///
/// ```text
/// relative_close[t] = (close - low) / (high - low) × 100
/// ```
///
/// - 100 % means close == high (strongest close).
/// - 0 % means close == low (weakest close).
/// - 50 % means close at midpoint.
///
/// Returns [`SignalValue::Unavailable`] when `high == low` (zero-range bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::RelativeClose;
/// use fin_primitives::signals::Signal;
/// let rc = RelativeClose::new("rc");
/// assert_eq!(rc.period(), 1);
/// ```
pub struct RelativeClose {
    name: String,
}

impl RelativeClose {
    /// Constructs a new `RelativeClose`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for RelativeClose {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        let rc = (bar.close - bar.low) / range * Decimal::ONE_HUNDRED;
        Ok(SignalValue::Scalar(rc))
    }

    fn reset(&mut self) {}
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
    fn test_rc_close_at_high() {
        let mut rc = RelativeClose::new("rc");
        let v = rc.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_rc_close_at_low() {
        let mut rc = RelativeClose::new("rc");
        let v = rc.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rc_close_at_midpoint() {
        let mut rc = RelativeClose::new("rc");
        let v = rc.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_rc_zero_range_unavailable() {
        let mut rc = RelativeClose::new("rc");
        let v = rc.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_rc_always_ready() {
        let rc = RelativeClose::new("rc");
        assert!(rc.is_ready());
    }
}
"""

cumulative_volume = """\
//! Cumulative Volume indicator — rolling N-bar sum of volume.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Cumulative Volume — rolling sum of traded volume over `period` bars.
///
/// ```text
/// cum_vol[t] = volume[t] + volume[t-1] + ... + volume[t-period+1]
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::CumulativeVolume;
/// use fin_primitives::signals::Signal;
/// let cv = CumulativeVolume::new("cv", 5).unwrap();
/// assert_eq!(cv.period(), 5);
/// ```
pub struct CumulativeVolume {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl CumulativeVolume {
    /// Constructs a new `CumulativeVolume`.
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

impl Signal for CumulativeVolume {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        self.sum += bar.volume;
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
    fn test_cv_period_0_error() { assert!(CumulativeVolume::new("cv", 0).is_err()); }

    #[test]
    fn test_cv_unavailable_before_period() {
        let mut cv = CumulativeVolume::new("cv", 3).unwrap();
        assert_eq!(cv.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(!cv.is_ready());
    }

    #[test]
    fn test_cv_sum_correct() {
        let mut cv = CumulativeVolume::new("cv", 3).unwrap();
        cv.update_bar(&bar("100")).unwrap();
        cv.update_bar(&bar("200")).unwrap();
        let v = cv.update_bar(&bar("300")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(600)));
    }

    #[test]
    fn test_cv_rolls_out_old() {
        let mut cv = CumulativeVolume::new("cv", 3).unwrap();
        cv.update_bar(&bar("100")).unwrap();
        cv.update_bar(&bar("200")).unwrap();
        cv.update_bar(&bar("300")).unwrap(); // sum=600
        let v = cv.update_bar(&bar("400")).unwrap(); // 100 leaves: 200+300+400=900
        assert_eq!(v, SignalValue::Scalar(dec!(900)));
    }

    #[test]
    fn test_cv_reset() {
        let mut cv = CumulativeVolume::new("cv", 2).unwrap();
        cv.update_bar(&bar("100")).unwrap();
        cv.update_bar(&bar("200")).unwrap();
        assert!(cv.is_ready());
        cv.reset();
        assert!(!cv.is_ready());
        assert_eq!(cv.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
"""

files = {
    "open_gap_pct": open_gap_pct,
    "candle_range_ma": candle_range_ma,
    "relative_close": relative_close,
    "cumulative_volume": cumulative_volume,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w") as f:
        f.write(content)
    print(f"wrote {path}")

print("done")
