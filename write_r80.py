import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. VolumeDeltaOscillator
volume_delta_oscillator = r'''//! Volume Delta Oscillator indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Difference between short-period and long-period volume SMA.
///
/// Positive: short-term volume above long-term baseline (surging activity).
/// Negative: short-term volume below long-term baseline (fading activity).
/// `short_period` must be less than `long_period`.
pub struct VolumeDeltaOscillator {
    short_period: usize,
    long_period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeDeltaOscillator {
    /// Creates a new `VolumeDeltaOscillator` with short and long periods.
    pub fn new(short_period: usize, long_period: usize) -> Result<Self, FinError> {
        if short_period == 0 || long_period <= short_period {
            return Err(FinError::InvalidPeriod(long_period));
        }
        Ok(Self {
            short_period,
            long_period,
            window: VecDeque::with_capacity(long_period),
            sum: Decimal::ZERO,
        })
    }
}

impl Signal for VolumeDeltaOscillator {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        self.window.push_back(bar.volume);
        self.sum += bar.volume;
        if self.window.len() > self.long_period {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old;
            }
        }
        if self.window.len() < self.long_period {
            return Ok(SignalValue::Unavailable);
        }

        let long_sma = self.sum / Decimal::from(self.long_period as u32);
        let short_sum: Decimal = self.window.iter().rev().take(self.short_period).sum();
        let short_sma = short_sum / Decimal::from(self.short_period as u32);
        Ok(SignalValue::Scalar(short_sma - long_sma))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.long_period }
    fn period(&self) -> usize { self.long_period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeDeltaOscillator" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(v: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(110),
            low: dec!(90),
            close: dec!(100),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vdo_equal_volumes_zero() {
        // Constant volume → short SMA = long SMA → oscillator = 0
        let mut sig = VolumeDeltaOscillator::new(2, 4).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        sig.update(&bar("1000")).unwrap();
        let v = sig.update(&bar("1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vdo_short_surge() {
        // Long window has lower volumes, short window has high volumes → positive
        let mut sig = VolumeDeltaOscillator::new(2, 4).unwrap();
        sig.update(&bar("500")).unwrap();
        sig.update(&bar("500")).unwrap();
        sig.update(&bar("2000")).unwrap();
        let v = sig.update(&bar("2000")).unwrap();
        // long_sma = (500+500+2000+2000)/4 = 1250, short_sma = (2000+2000)/2 = 2000
        // oscillator = 2000 - 1250 = 750
        assert_eq!(v, SignalValue::Scalar(dec!(750)));
    }
}
'''

# 2. HigherHighLowerLow
higher_high_lower_low = r'''//! Higher High Lower Low indicator.

use rust_decimal::Decimal;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Detects bar pattern: +1 if higher high AND lower low (outside bar),
/// -1 if lower high AND higher low (inside bar), 0 otherwise.
///
/// Outside bars (expanding range): indicate volatility breakout.
/// Inside bars (contracting range): indicate consolidation / indecision.
/// Returns Unavailable until the second bar.
pub struct HigherHighLowerLow {
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
}

impl HigherHighLowerLow {
    /// Creates a new `HigherHighLowerLow` indicator.
    pub fn new() -> Self {
        Self { prev_high: None, prev_low: None }
    }
}

impl Default for HigherHighLowerLow {
    fn default() -> Self { Self::new() }
}

impl Signal for HigherHighLowerLow {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let result = if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            let outside = bar.high > ph && bar.low < pl;
            let inside = bar.high < ph && bar.low > pl;
            let val: i32 = if outside { 1 } else if inside { -1 } else { 0 };
            SignalValue::Scalar(Decimal::from(val))
        } else {
            SignalValue::Unavailable
        };
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        Ok(result)
    }

    fn is_ready(&self) -> bool { self.prev_high.is_some() }
    fn period(&self) -> usize { 2 }
    fn reset(&mut self) { self.prev_high = None; self.prev_low = None; }
    fn name(&self) -> &str { "HigherHighLowerLow" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_hhll_outside_bar() {
        let mut sig = HigherHighLowerLow::new();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("115", "85")).unwrap(); // higher high AND lower low
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_hhll_inside_bar() {
        let mut sig = HigherHighLowerLow::new();
        sig.update(&bar("115", "85")).unwrap();
        let v = sig.update(&bar("110", "90")).unwrap(); // lower high AND higher low
        assert_eq!(v, SignalValue::Scalar(dec!(-1)));
    }

    #[test]
    fn test_hhll_neutral() {
        let mut sig = HigherHighLowerLow::new();
        sig.update(&bar("110", "90")).unwrap();
        let v = sig.update(&bar("115", "91")).unwrap(); // higher high but NOT lower low
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 3. RollingReturnKurtosis
rolling_return_kurtosis = r'''//! Rolling Return Kurtosis indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Excess kurtosis of close returns over the rolling period.
///
/// Measures tail heaviness relative to a normal distribution.
/// Positive (leptokurtic): fat tails, extreme returns more frequent.
/// Negative (platykurtic): thin tails, returns more uniformly distributed.
/// Requires at least 4 bars (period >= 4).
pub struct RollingReturnKurtosis {
    period: usize,
    prev_close: Option<Decimal>,
    returns: VecDeque<f64>,
}

impl RollingReturnKurtosis {
    /// Creates a new `RollingReturnKurtosis` with the given period (min 4).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 4 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, returns: VecDeque::with_capacity(period) })
    }
}

impl Signal for RollingReturnKurtosis {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            if !pc.is_zero() {
                if let (Some(c), Some(p)) = (bar.close.to_f64(), pc.to_f64()) {
                    let ret = (c - p) / p;
                    self.returns.push_back(ret);
                    if self.returns.len() > self.period {
                        self.returns.pop_front();
                    }
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.returns.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = self.period as f64;
        let vals: Vec<f64> = self.returns.iter().cloned().collect();
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        if var == 0.0 {
            return Ok(SignalValue::Scalar(Decimal::ZERO));
        }
        let m4 = vals.iter().map(|v| (v - mean).powi(4)).sum::<f64>() / n;
        let kurtosis = m4 / (var * var) - 3.0; // excess kurtosis

        match Decimal::from_f64_retain(kurtosis) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.returns.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.returns.clear(); }
    fn name(&self) -> &str { "RollingReturnKurtosis" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str) -> BarInput {
        BarInput {
            open: c.parse().unwrap(),
            high: c.parse().unwrap(),
            low: c.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_rrk_constant_prices_zero() {
        // Constant price → variance = 0 → kurtosis = 0
        let mut sig = RollingReturnKurtosis::new(4).unwrap();
        for _ in 0..5 {
            sig.update(&bar("100")).unwrap();
        }
        let v = sig.update(&bar("100")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_rrk_not_ready() {
        let mut sig = RollingReturnKurtosis::new(4).unwrap();
        for _ in 0..4 {
            assert_eq!(sig.update(&bar("100")).unwrap(), SignalValue::Unavailable);
        }
    }
}
'''

# 4. VolumeToRangeRatio
volume_to_range_ratio = r'''//! Volume to Range Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling average of `volume / (high - low)`.
///
/// Measures how much volume is required to move price per unit of range.
/// High values: heavy volume for each unit of range (inefficient, choppy).
/// Low values: price moves wide ranges on light volume (efficient, trending).
/// Bars with zero range are skipped.
pub struct VolumeToRangeRatio {
    period: usize,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeToRangeRatio {
    /// Creates a new `VolumeToRangeRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for VolumeToRangeRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let range = bar.high - bar.low;
        if !range.is_zero() {
            let ratio = bar.volume / range;
            self.window.push_back(ratio);
            self.sum += ratio;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let len = Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(self.sum / len))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeToRangeRatio" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, v: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vtrr_basic() {
        // volume=1000, range=20 → ratio=50
        let mut sig = VolumeToRangeRatio::new(2).unwrap();
        sig.update(&bar("110", "90", "1000")).unwrap();
        let v = sig.update(&bar("110", "90", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(50)));
    }

    #[test]
    fn test_vtrr_mixed() {
        // bar1: vol=1000, range=10 → ratio=100; bar2: vol=1000, range=20 → ratio=50; avg=75
        let mut sig = VolumeToRangeRatio::new(2).unwrap();
        sig.update(&bar("110", "100", "1000")).unwrap(); // range=10, ratio=100
        let v = sig.update(&bar("110", "90", "1000")).unwrap(); // range=20, ratio=50, avg=75
        assert_eq!(v, SignalValue::Scalar(dec!(75)));
    }
}
'''

files = [
    ("volume_delta_oscillator.rs", volume_delta_oscillator),
    ("higher_high_lower_low.rs", higher_high_lower_low),
    ("rolling_return_kurtosis.rs", rolling_return_kurtosis),
    ("volume_to_range_ratio.rs", volume_to_range_ratio),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
