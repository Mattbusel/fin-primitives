import os

base = "src/signals/indicators"

consecutive_volume_growth = """\
//! Consecutive Volume Growth indicator -- streak of increasing-volume bars.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Consecutive Volume Growth -- the number of consecutive bars where volume
/// exceeded the previous bar's volume.
///
/// Resets to 0 when a bar's volume does not exceed the prior bar's volume.
/// Useful for detecting unusual sustained volume surges.
///
/// ```text
/// streak[t] = streak[t-1] + 1  if volume[t] > volume[t-1]
///           = 0                 otherwise
/// ```
///
/// Returns 0 on the first bar (no prior bar to compare).
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::ConsecutiveVolumeGrowth;
/// use fin_primitives::signals::Signal;
/// let cvg = ConsecutiveVolumeGrowth::new("cvg");
/// assert_eq!(cvg.period(), 1);
/// ```
pub struct ConsecutiveVolumeGrowth {
    name: String,
    prev_volume: Option<Decimal>,
    streak: u32,
}

impl ConsecutiveVolumeGrowth {
    /// Constructs a new `ConsecutiveVolumeGrowth`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), prev_volume: None, streak: 0 }
    }
}

impl Signal for ConsecutiveVolumeGrowth {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { true }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.streak = match self.prev_volume {
            Some(pv) if bar.volume > pv => self.streak + 1,
            _ => 0,
        };
        self.prev_volume = Some(bar.volume);
        Ok(SignalValue::Scalar(Decimal::from(self.streak)))
    }

    fn reset(&mut self) {
        self.prev_volume = None;
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
    fn test_cvg_first_bar_is_zero() {
        let mut cvg = ConsecutiveVolumeGrowth::new("cvg");
        let v = cvg.update_bar(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cvg_growing_streak() {
        let mut cvg = ConsecutiveVolumeGrowth::new("cvg");
        cvg.update_bar(&bar("100")).unwrap();    // streak=0
        cvg.update_bar(&bar("200")).unwrap();    // streak=1
        cvg.update_bar(&bar("300")).unwrap();    // streak=2
        let v = cvg.update_bar(&bar("400")).unwrap(); // streak=3
        assert_eq!(v, SignalValue::Scalar(dec!(3)));
    }

    #[test]
    fn test_cvg_resets_on_flat() {
        let mut cvg = ConsecutiveVolumeGrowth::new("cvg");
        cvg.update_bar(&bar("100")).unwrap(); // streak=0
        cvg.update_bar(&bar("200")).unwrap(); // streak=1
        cvg.update_bar(&bar("200")).unwrap(); // not growing -> streak=0
        let v = cvg.update_bar(&bar("300")).unwrap(); // streak=1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_cvg_resets_on_decrease() {
        let mut cvg = ConsecutiveVolumeGrowth::new("cvg");
        cvg.update_bar(&bar("500")).unwrap(); // streak=0
        cvg.update_bar(&bar("600")).unwrap(); // streak=1
        let v = cvg.update_bar(&bar("400")).unwrap(); // decrease -> streak=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_cvg_reset() {
        let mut cvg = ConsecutiveVolumeGrowth::new("cvg");
        cvg.update_bar(&bar("100")).unwrap();
        cvg.update_bar(&bar("200")).unwrap();
        cvg.reset();
        let v = cvg.update_bar(&bar("300")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
"""

volume_density = """\
//! Volume Density indicator -- rolling average of volume per unit price range.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Volume Density -- rolling average of `volume / (high - low)` over `period` bars.
///
/// Measures how much volume occurs per unit of price range. Higher values indicate
/// dense trading at a tight range (often near support/resistance). Zero-range bars
/// are excluded from the average.
///
/// ```text
/// density[t] = volume[t] / (high[t] - low[t])   (if high != low)
/// vd[t]      = SMA(density, period)              (counting only valid bars)
/// ```
///
/// Returns [`SignalValue::Unavailable`] until `period` valid bars have been accumulated.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::VolumeDensity;
/// use fin_primitives::signals::Signal;
/// let vd = VolumeDensity::new("vd", 10).unwrap();
/// assert_eq!(vd.period(), 10);
/// ```
pub struct VolumeDensity {
    name: String,
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeDensity {
    /// Constructs a new `VolumeDensity`.
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

impl Signal for VolumeDensity {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.period }
    fn is_ready(&self) -> bool { self.window.len() >= self.period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        let density = bar.volume / range;
        self.window.push_back(density);
        self.sum += density;
        if self.window.len() > self.period {
            if let Some(old) = self.window.pop_front() { self.sum -= old; }
        }
        if self.window.len() < self.period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        Ok(SignalValue::Scalar(self.sum / Decimal::from(self.period as u32)))
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

    fn bar(h: &str, l: &str, vol: &str) -> OhlcvBar {
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        let mp = Price::new((hp.value() + lp.value()) / Decimal::TWO).unwrap();
        let v = Quantity::new(vol.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: mp, high: hp, low: lp, close: mp, volume: v,
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_vd_period_0_error() { assert!(VolumeDensity::new("vd", 0).is_err()); }

    #[test]
    fn test_vd_zero_range_unavailable() {
        let mut vd = VolumeDensity::new("vd", 1).unwrap();
        let hp = Price::new(dec!(100)).unwrap();
        let v = Quantity::new(dec!(1000)).unwrap();
        let b = OhlcvBar {
            symbol: crate::types::Symbol::new("X").unwrap(),
            open: hp, high: hp, low: hp, close: hp, volume: v,
            ts_open: crate::types::NanoTimestamp::new(0),
            ts_close: crate::types::NanoTimestamp::new(1),
            tick_count: 1,
        };
        assert_eq!(vd.update_bar(&b).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vd_unavailable_before_period() {
        let mut vd = VolumeDensity::new("vd", 3).unwrap();
        assert_eq!(vd.update_bar(&bar("110", "90", "2000")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_vd_constant_density() {
        // range=20, vol=2000 -> density=100 per bar
        let mut vd = VolumeDensity::new("vd", 3).unwrap();
        vd.update_bar(&bar("110", "90", "2000")).unwrap();
        vd.update_bar(&bar("110", "90", "2000")).unwrap();
        let v = vd.update_bar(&bar("110", "90", "2000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_vd_reset() {
        let mut vd = VolumeDensity::new("vd", 2).unwrap();
        vd.update_bar(&bar("110", "90", "2000")).unwrap();
        vd.update_bar(&bar("110", "90", "2000")).unwrap();
        assert!(vd.is_ready());
        vd.reset();
        assert!(!vd.is_ready());
    }
}
"""

open_high_ratio = """\
//! Open-to-High Ratio indicator -- how far the high extends from the open.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;

/// Open-to-High Ratio -- the upper extension of the bar from the open,
/// expressed as a percentage of the bar's total range.
///
/// ```text
/// ohr[t] = (high - open) / (high - low) * 100
/// ```
///
/// - 100% → high was far above open (strong upper breakout from open)
/// - 0%   → open was at the high (no upper extension from open)
///
/// Returns [`SignalValue::Unavailable`] if `high == low` (zero-range bar).
/// Ready after the first bar.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::OpenHighRatio;
/// use fin_primitives::signals::Signal;
/// let ohr = OpenHighRatio::new("ohr");
/// assert_eq!(ohr.period(), 1);
/// ```
pub struct OpenHighRatio {
    name: String,
    ready: bool,
}

impl OpenHighRatio {
    /// Constructs a new `OpenHighRatio`.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ready: false }
    }
}

impl Signal for OpenHighRatio {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { 1 }
    fn is_ready(&self) -> bool { self.ready }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.ready = true;
        let range = bar.high - bar.low;
        if range.is_zero() { return Ok(SignalValue::Unavailable); }
        Ok(SignalValue::Scalar((bar.high - bar.open) / range * Decimal::ONE_HUNDRED))
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

    fn bar(o: &str, h: &str, l: &str) -> OhlcvBar {
        let op = Price::new(o.parse().unwrap()).unwrap();
        let hp = Price::new(h.parse().unwrap()).unwrap();
        let lp = Price::new(l.parse().unwrap()).unwrap();
        OhlcvBar {
            symbol: Symbol::new("X").unwrap(),
            open: op, high: hp, low: lp, close: op,
            volume: Quantity::zero(),
            ts_open: NanoTimestamp::new(0),
            ts_close: NanoTimestamp::new(1),
            tick_count: 1,
        }
    }

    #[test]
    fn test_ohr_open_at_low_is_100() {
        // open=90 (at the low), high=110, low=90 -> (110-90)/(110-90)*100 = 100
        let mut ohr = OpenHighRatio::new("ohr");
        let v = ohr.update_bar(&bar("90", "110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(100)));
    }

    #[test]
    fn test_ohr_open_at_high_is_0() {
        // open=110 (at the high), high=110, low=90 -> (110-110)/(110-90)*100 = 0
        let mut ohr = OpenHighRatio::new("ohr");
        let v = ohr.update_bar(&bar("110", "110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_ohr_open_at_midpoint_is_50() {
        // open=100, high=110, low=90 -> (110-100)/(110-90)*100 = 50
        let mut ohr = OpenHighRatio::new("ohr");
        let v = ohr.update_bar(&bar("100", "110", "90")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_ohr_zero_range_unavailable() {
        let mut ohr = OpenHighRatio::new("ohr");
        let v = ohr.update_bar(&bar("100", "100", "100")).unwrap();
        assert_eq!(v, SignalValue::Unavailable);
    }

    #[test]
    fn test_ohr_ready_after_first_bar() {
        let mut ohr = OpenHighRatio::new("ohr");
        assert!(!ohr.is_ready());
        ohr.update_bar(&bar("100", "110", "90")).unwrap();
        assert!(ohr.is_ready());
    }

    #[test]
    fn test_ohr_reset() {
        let mut ohr = OpenHighRatio::new("ohr");
        ohr.update_bar(&bar("100", "110", "90")).unwrap();
        ohr.reset();
        assert!(!ohr.is_ready());
    }
}
"""

price_oscillator2 = """\
//! Price Oscillator 2 indicator -- fast EMA minus slow SMA, percentage-based.

use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// Price Oscillator 2 -- the percentage difference between a fast EMA and a slow SMA.
///
/// Combines the smoothing of EMA with the simplicity of SMA for a trend/momentum signal.
///
/// ```text
/// fast_ema[t] = EMA(close, fast_period)
/// slow_sma[t] = SMA(close, slow_period)
/// osc[t]      = (fast_ema - slow_sma) / slow_sma * 100
/// ```
///
/// Positive values indicate the EMA is above the SMA (bullish);
/// negative values indicate the EMA is below the SMA (bearish).
///
/// Returns [`SignalValue::Unavailable`] until `slow_period` bars have been seen.
///
/// # Example
/// ```rust
/// use fin_primitives::signals::indicators::PriceOscillator2;
/// use fin_primitives::signals::Signal;
/// let po = PriceOscillator2::new("po", 12, 26).unwrap();
/// assert_eq!(po.period(), 26);
/// ```
pub struct PriceOscillator2 {
    name: String,
    fast_period: usize,
    slow_period: usize,
    ema: Option<Decimal>,
    ema_k: Decimal,
    sma_window: VecDeque<Decimal>,
    sma_sum: Decimal,
}

impl PriceOscillator2 {
    /// Constructs a new `PriceOscillator2`.
    ///
    /// # Errors
    /// Returns [`FinError::InvalidPeriod`] if `fast_period == 0`, `slow_period == 0`,
    /// or `fast_period >= slow_period`.
    pub fn new(name: impl Into<String>, fast_period: usize, slow_period: usize) -> Result<Self, FinError> {
        if fast_period == 0 { return Err(FinError::InvalidPeriod(fast_period)); }
        if slow_period == 0 { return Err(FinError::InvalidPeriod(slow_period)); }
        if fast_period >= slow_period { return Err(FinError::InvalidPeriod(fast_period)); }
        #[allow(clippy::cast_possible_truncation)]
        let k = Decimal::TWO / Decimal::from((fast_period + 1) as u32);
        Ok(Self {
            name: name.into(),
            fast_period,
            slow_period,
            ema: None,
            ema_k: k,
            sma_window: VecDeque::with_capacity(slow_period),
            sma_sum: Decimal::ZERO,
        })
    }
}

impl Signal for PriceOscillator2 {
    fn name(&self) -> &str { &self.name }
    fn period(&self) -> usize { self.slow_period }
    fn is_ready(&self) -> bool { self.sma_window.len() >= self.slow_period }

    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        // Update EMA
        self.ema = Some(match self.ema {
            None => bar.close,
            Some(prev) => self.ema_k * bar.close + (Decimal::ONE - self.ema_k) * prev,
        });
        // Update SMA
        self.sma_window.push_back(bar.close);
        self.sma_sum += bar.close;
        if self.sma_window.len() > self.slow_period {
            if let Some(old) = self.sma_window.pop_front() { self.sma_sum -= old; }
        }
        if self.sma_window.len() < self.slow_period { return Ok(SignalValue::Unavailable); }
        #[allow(clippy::cast_possible_truncation)]
        let slow_sma = self.sma_sum / Decimal::from(self.slow_period as u32);
        if slow_sma.is_zero() { return Ok(SignalValue::Unavailable); }
        let fast_ema = self.ema.unwrap_or(bar.close);
        Ok(SignalValue::Scalar((fast_ema - slow_sma) / slow_sma * Decimal::ONE_HUNDRED))
    }

    fn reset(&mut self) {
        self.ema = None;
        self.sma_window.clear();
        self.sma_sum = Decimal::ZERO;
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
    fn test_po2_invalid_periods() {
        assert!(PriceOscillator2::new("po", 0, 5).is_err());
        assert!(PriceOscillator2::new("po", 5, 0).is_err());
        assert!(PriceOscillator2::new("po", 5, 5).is_err()); // fast >= slow
        assert!(PriceOscillator2::new("po", 10, 5).is_err()); // fast > slow
    }

    #[test]
    fn test_po2_unavailable_before_slow_period() {
        let mut po = PriceOscillator2::new("po", 2, 5).unwrap();
        assert_eq!(po.update_bar(&bar("100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_po2_flat_price_is_zero() {
        // When all prices are the same, EMA == SMA -> oscillator = 0
        let mut po = PriceOscillator2::new("po", 2, 5).unwrap();
        for _ in 0..5 { po.update_bar(&bar("100")).unwrap(); }
        let v = po.update_bar(&bar("100")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s.abs() < dec!(0.001), "flat prices, oscillator near 0, got {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_po2_rising_prices_positive() {
        // With rising prices, fast EMA rises faster than slow SMA -> positive oscillator
        let mut po = PriceOscillator2::new("po", 2, 4).unwrap();
        for i in 0u32..4 { po.update_bar(&bar(&(100 + i * 5).to_string())).unwrap(); }
        // After warmup: fast EMA will have responded more to recent price rises
        let v = po.update_bar(&bar("125")).unwrap();
        if let SignalValue::Scalar(s) = v {
            assert!(s > dec!(0), "rising prices, fast EMA > slow SMA, got {s}");
        } else {
            panic!("expected Scalar");
        }
    }

    #[test]
    fn test_po2_reset() {
        let mut po = PriceOscillator2::new("po", 2, 4).unwrap();
        for _ in 0..5 { po.update_bar(&bar("100")).unwrap(); }
        assert!(po.is_ready());
        po.reset();
        assert!(!po.is_ready());
    }
}
"""

files = {
    "consecutive_volume_growth": consecutive_volume_growth,
    "volume_density": volume_density,
    "open_high_ratio": open_high_ratio,
    "price_oscillator2": price_oscillator2,
}

for name, content in files.items():
    path = os.path.join(base, f"{name}.rs")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    print(f"wrote {path}")

print("done")
