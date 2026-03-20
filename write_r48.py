import os

base = "src/signals/indicators"

ema_band_width = """\
//! EMA Band Width indicator -- fast/slow EMA spread as a percentage.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// EMA Band Width -- width between a fast EMA and a slow EMA expressed as a
/// percentage of the slow EMA.
///
/// ```text
/// band_width[t] = (fast_ema - slow_ema) / slow_ema x 100
/// ```
///
/// Positive values indicate the fast EMA is above the slow EMA (uptrend);
/// negative values indicate it is below (downtrend). The magnitude indicates
/// how wide the spread is relative to the slow baseline.
///
/// Returns [`SignalValue::Unavailable`] until the slow (larger) EMA has warmed up.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::EmaBandWidth;
/// use fin_primitives::signals::Signal;
/// let ebw = EmaBandWidth::new("ebw", 12, 26).unwrap();
/// assert_eq!(ebw.period(), 26);
/// ```
pub struct EmaBandWidth {
    name: String,
    fast_period: usize,
    slow_period: usize,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    fast_k: Decimal,
    slow_k: Decimal,
    bars: usize,
}

impl EmaBandWidth {
    /// Constructs a new `EmaBandWidth`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if either period is 0 or `fast >= slow`.
    pub fn new(name: impl Into<String>, fast: usize, slow: usize) -> Result<Self, FinError> {
        if fast == 0 { return Err(FinError::InvalidPeriod(fast)); }
        if slow == 0 || fast >= slow { return Err(FinError::InvalidPeriod(slow)); }
        #[allow(clippy::cast_possible_truncation)]
        let fast_k = Decimal::from(2u32) / (Decimal::from(fast as u32) + Decimal::ONE);
        #[allow(clippy::cast_possible_truncation)]
        let slow_k = Decimal::from(2u32) / (Decimal::from(slow as u32) + Decimal::ONE);
        Ok(Self {
            name: name.into(),
            fast_period: fast,
            slow_period: slow,
            fast_ema: None,
            slow_ema: None,
            fast_k,
            slow_k,
            bars: 0,
        })
    }

    /// Returns the fast EMA period.
    pub fn fast_period(&self) -> usize { self.fast_period }
    /// Returns the slow EMA period.
    pub fn slow_period(&self) -> usize { self.slow_period }
}

impl Signal for EmaBandWidth {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.slow_period }
    fn is_ready(&self) -> bool { self.bars > self.slow_period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.bars += 1;
        let close = bar.close;

        self.fast_ema = Some(match self.fast_ema {
            None => close,
            Some(prev) => prev + self.fast_k * (close - prev),
        });
        self.slow_ema = Some(match self.slow_ema {
            None => close,
            Some(prev) => prev + self.slow_k * (close - prev),
        });

        if self.bars <= self.slow_period {
            return Ok(SignalValue::Unavailable);
        }

        let fast = self.fast_ema.unwrap_or(Decimal::ZERO);
        let slow = self.slow_ema.unwrap_or(Decimal::ZERO);
        if slow.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar((fast - slow) / slow * Decimal::ONE_HUNDRED))
    }

    fn reset(&mut self) {
        self.fast_ema = None;
        self.slow_ema = None;
        self.bars = 0;
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
    fn test_ebw_invalid_periods() {
        assert!(EmaBandWidth::new("e", 0, 26).is_err());
        assert!(EmaBandWidth::new("e", 26, 12).is_err()); // fast >= slow
        assert!(EmaBandWidth::new("e", 26, 26).is_err()); // fast == slow
    }

    #[test]
    fn test_ebw_unavailable_before_slow_period() {
        let mut e = EmaBandWidth::new("e", 3, 5).unwrap();
        for _ in 0..5 {
            assert_eq!(e.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }

    #[test]
    fn test_ebw_constant_price_zero_width() {
        let mut e = EmaBandWidth::new("e", 3, 5).unwrap();
        // With constant price, both EMAs converge to same value -> width = 0
        for _ in 0..20 {
            e.update_bar(&bar("100")).unwrap();
        }
        if let SignalValue::Scalar(v) = e.update_bar(&bar("100")).unwrap() {
            assert!(v.abs() < dec!(0.0001), "expected ~0, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ebw_uptrend_positive() {
        let mut e = EmaBandWidth::new("e", 3, 5).unwrap();
        // Rising prices: fast EMA responds faster, so fast > slow
        for i in 0u32..20 {
            e.update_bar(&bar(&(100 + i).to_string())).unwrap();
        }
        if let SignalValue::Scalar(v) = e.update_bar(&bar("120")).unwrap() {
            assert!(v > dec!(0), "expected positive band width in uptrend, got {v}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_ebw_reset() {
        let mut e = EmaBandWidth::new("e", 3, 5).unwrap();
        for _ in 0..20 { e.update_bar(&bar("100")).unwrap(); }
        assert!(e.is_ready());
        e.reset();
        assert!(!e.is_ready());
    }
}
"""

consecutive_new_highs = """\
//! Consecutive New Highs indicator.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Consecutive New Highs -- counts the current streak of bars making a new N-period high.
///
/// Each bar, the indicator checks whether the current `high` exceeds the rolling maximum
/// of the previous `period` bars. If it does, the streak counter increments; otherwise
/// it resets to zero.
///
/// Returns [`SignalValue::Unavailable`] until `period + 1` bars have been seen
/// (need a full window of prior bars before making the comparison).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ConsecutiveNewHighs;
/// use fin_primitives::signals::Signal;
/// let cnh = ConsecutiveNewHighs::new("cnh", 5).unwrap();
/// assert_eq!(cnh.period(), 5);
/// ```
pub struct ConsecutiveNewHighs {
    name: String,
    period: usize,
    window: VecDeque<Decimal>, // rolling window of prior highs
    streak: u32,
}

impl ConsecutiveNewHighs {
    /// Constructs a new `ConsecutiveNewHighs`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `period == 0`.
    pub fn new(name: impl Into<String>, period: usize) -> Result<Self, FinError> {
        if period == 0 { return Err(FinError::InvalidPeriod(period)); }
        Ok(Self {
            name: name.into(),
            period,
            window: VecDeque::with_capacity(period),
            streak: 0,
        })
    }
}

impl Signal for ConsecutiveNewHighs {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if self.window.len() < self.period {
            self.window.push_back(bar.high);
            return Ok(SignalValue::Unavailable);
        }

        // window is full: compare current high to max of window
        let prev_max = self.window.iter().copied().fold(Decimal::MIN, Decimal::max);
        if bar.high > prev_max {
            self.streak += 1;
        } else {
            self.streak = 0;
        }

        // slide window forward
        self.window.pop_front();
        self.window.push_back(bar.high);

        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.window.clear();
        self.streak = 0;
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
    fn test_cnh_period_0_error() { assert!(ConsecutiveNewHighs::new("c", 0).is_err()); }

    #[test]
    fn test_cnh_unavailable_during_warmup() {
        let mut c = ConsecutiveNewHighs::new("c", 3).unwrap();
        assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        // 4th bar is first comparison
        let v = c.update_bar(&bar("101")).unwrap();
        assert!(v.is_scalar());
    }

    #[test]
    fn test_cnh_new_high_increments_streak() {
        let mut c = ConsecutiveNewHighs::new("c", 3).unwrap();
        // seed window with [100, 100, 100]
        for _ in 0..3 { c.update_bar(&bar("100")).unwrap(); }
        // 101 > max(100) -> streak=1
        assert_eq!(c.update_bar(&bar("101")).unwrap(), SignalValue::Scalar(dec!(1)));
        // 102 > max(100, 100, 101) = 101 -> streak=2
        assert_eq!(c.update_bar(&bar("102")).unwrap(), SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_cnh_no_new_high_resets_streak() {
        let mut c = ConsecutiveNewHighs::new("c", 3).unwrap();
        for _ in 0..3 { c.update_bar(&bar("100")).unwrap(); }
        c.update_bar(&bar("105")).unwrap(); // streak=1
        // 95 <= max of window (which contains 105 now) -> reset
        let v = c.update_bar(&bar("95")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cnh_reset() {
        let mut c = ConsecutiveNewHighs::new("c", 3).unwrap();
        for _ in 0..5 { c.update_bar(&bar("100")).unwrap(); }
        assert!(c.is_ready());
        c.reset();
        assert!(!c.is_ready());
        assert_eq!(c.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
"""

volume_direction_ratio = """\
//! Volume Direction Ratio indicator -- estimated buy/sell imbalance from price action.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Volume Direction Ratio -- approximates net buying pressure as a fraction of total volume.
///
/// Since intra-bar tick direction is unavailable at the OHLCV level, this indicator uses
/// the close position within the bar range as a proxy for buy vs. sell volume:
///
/// ```text
/// close_pct = (close - low) / (high - low)   (0 = all selling, 1 = all buying)
/// vdr[t]    = 2 * close_pct - 1              (rescaled to [-1, +1])
/// ```
///
/// A value of +1 means the close was at the high (fully bullish); -1 means at the low.
/// Returns [`SignalValue::Unavailable`] when `high == low` (zero-range bar).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeDirectionRatio;
/// use fin_primitives::signals::Signal;
/// let vdr = VolumeDirectionRatio::new("vdr");
/// assert_eq!(vdr.period(), 1);
/// ```
pub struct VolumeDirectionRatio {
    name: String,
}

impl VolumeDirectionRatio {
    /// Constructs a new `VolumeDirectionRatio`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Signal for VolumeDirectionRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        let close_pct = (bar.close - bar.low) / range;
        let vdr = Decimal::from(2u32) * close_pct - Decimal::ONE;
        Ok(SignalValue::Scalar(vdr))
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
    fn test_vdr_close_at_high_is_plus_one() {
        let mut vdr = VolumeDirectionRatio::new("vdr");
        let v = vdr.update_bar(&bar("110", "90", "110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_vdr_close_at_low_is_minus_one() {
        let mut vdr = VolumeDirectionRatio::new("vdr");
        let v = vdr.update_bar(&bar("110", "90", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_vdr_close_at_midpoint_is_zero() {
        let mut vdr = VolumeDirectionRatio::new("vdr");
        let v = vdr.update_bar(&bar("110", "90", "100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vdr_zero_range_unavailable() {
        let mut vdr = VolumeDirectionRatio::new("vdr");
        let v = vdr.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_vdr_always_ready() {
        let vdr = VolumeDirectionRatio::new("vdr");
        assert!(vdr.is_ready());
    }
}
"""

price_change_pct = """\
//! Price Change Percent indicator -- bar-over-bar close change as a percentage.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Price Change Percent -- percentage change in close from the previous bar.
///
/// ```text
/// pct_change[t] = (close[t] - close[t-1]) / close[t-1] x 100
/// ```
///
/// Returns [`SignalValue::Unavailable`] on the first bar (no prior close).
/// Returns [`SignalValue::Unavailable`] if the prior close is zero.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceChangePct;
/// use fin_primitives::signals::Signal;
/// let pcp = PriceChangePct::new("pcp");
/// assert_eq!(pcp.period(), 1);
/// ```
pub struct PriceChangePct {
    name: String,
    prev_close: Option<Decimal>,
}

impl PriceChangePct {
    /// Constructs a new `PriceChangePct`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_close: None }
    }
}

impl Signal for PriceChangePct {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.prev_close.is_some() }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = match self.prev_close {
            None => SignalValue::Unavailable,
            Some(pc) if pc.is_zero() => SignalValue::Unavailable,
            Some(pc) => {
                let pct = (bar.close - pc) / pc * Decimal::ONE_HUNDRED;
                SignalValue::Scalar(pct)
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
    fn test_pcp_first_bar_unavailable() {
        let mut pcp = PriceChangePct::new("pcp");
        assert_eq!(pcp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
        assert!(pcp.is_ready());
    }

    #[test]
    fn test_pcp_gain() {
        let mut pcp = PriceChangePct::new("pcp");
        pcp.update_bar(&bar("100")).unwrap();
        let v = pcp.update_bar(&bar("110")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(10)));
    }

    #[test]
    fn test_pcp_loss() {
        let mut pcp = PriceChangePct::new("pcp");
        pcp.update_bar(&bar("100")).unwrap();
        let v = pcp.update_bar(&bar("90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(-10)));
    }

    #[test]
    fn test_pcp_no_change() {
        let mut pcp = PriceChangePct::new("pcp");
        pcp.update_bar(&bar("100")).unwrap();
        let v = pcp.update_bar(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_pcp_reset() {
        let mut pcp = PriceChangePct::new("pcp");
        pcp.update_bar(&bar("100")).unwrap();
        assert!(pcp.is_ready());
        pcp.reset();
        assert!(!pcp.is_ready());
        assert_eq!(pcp.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }
}
"""

files = {
    "ema_band_width": ema_band_width,
    "consecutive_new_highs": consecutive_new_highs,
    "volume_direction_ratio": volume_direction_ratio,
    "price_change_pct": price_change_pct,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
