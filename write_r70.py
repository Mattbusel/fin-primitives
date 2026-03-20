import os

base = r"R:\workspaces\fin-primitives\src\signals\indicators"

# 1. LowerLowCount
lower_low_count = r'''//! Lower Low Count indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling count of bars where `low < prior bar low`.
///
/// Measures downside momentum: higher counts indicate persistent downward price exploration.
pub struct LowerLowCount {
    period: usize,
    prev_low: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl LowerLowCount {
    /// Creates a new `LowerLowCount` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_low: None, window: VecDeque::with_capacity(period), count: 0 })
    }
}

impl Signal for LowerLowCount {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pl) = self.prev_low {
            let ll: u8 = if bar.low < pl { 1 } else { 0 };
            self.window.push_back(ll);
            self.count += ll as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_low = Some(bar.low);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(Decimal::from(self.count as u32)))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_low = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "LowerLowCount" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(l: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(110),
            low: l.parse().unwrap(),
            close: dec!(100),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_lower_low_count_all_lower() {
        let mut sig = LowerLowCount::new(2).unwrap();
        sig.update(&bar("100")).unwrap(); // seeds prev_low=100
        sig.update(&bar("95")).unwrap(); // 95<100 ✓
        let v = sig.update(&bar("90")).unwrap(); // 90<95 ✓ → count=2
        assert_eq!(v, SignalValue::Scalar(dec!(2)));
    }

    #[test]
    fn test_lower_low_count_none_lower() {
        let mut sig = LowerLowCount::new(2).unwrap();
        sig.update(&bar("90")).unwrap(); // seeds prev_low=90
        sig.update(&bar("95")).unwrap(); // 95>90 ✗
        let v = sig.update(&bar("100")).unwrap(); // 100>95 ✗ → count=0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 2. TypicalPriceDeviation
typical_price_deviation = r'''//! Typical Price Deviation indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};
use rust_decimal::prelude::ToPrimitive;

/// Rolling standard deviation of typical price `(high + low + close) / 3`.
///
/// Measures volatility using the typical price rather than just the close,
/// giving equal weight to the full bar's trading range.
pub struct TypicalPriceDeviation {
    period: usize,
    window: VecDeque<Decimal>,
}

impl TypicalPriceDeviation {
    /// Creates a new `TypicalPriceDeviation` with the given period (min 2).
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period < 2 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, window: VecDeque::with_capacity(period) })
    }
}

impl Signal for TypicalPriceDeviation {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        let tp = (bar.high + bar.low + bar.close) / Decimal::from(3u32);
        self.window.push_back(tp);
        if self.window.len() > self.period {
            self.window.pop_front();
        }
        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let vals: Vec<f64> = self.window.iter()
            .filter_map(|v| v.to_f64())
            .collect();
        if vals.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }

        let n = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n;
        let var = vals.iter().map(|v| { let d = v - mean; d * d }).sum::<f64>() / (n - 1.0);
        let std_dev = var.sqrt();

        match Decimal::from_f64_retain(std_dev) {
            Some(v) => Ok(SignalValue::Scalar(v)),
            None => Ok(SignalValue::Unavailable),
        }
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.window.clear(); }
    fn name(&self) -> &str { "TypicalPriceDeviation" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(h: &str, l: &str, c: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: h.parse().unwrap(),
            low: l.parse().unwrap(),
            close: c.parse().unwrap(),
            volume: dec!(1000),
        }
    }

    #[test]
    fn test_typical_price_deviation_not_ready() {
        let mut sig = TypicalPriceDeviation::new(3).unwrap();
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
        assert_eq!(sig.update(&bar("110", "90", "100")).unwrap(), SignalValue::Unavailable);
    }

    #[test]
    fn test_typical_price_deviation_constant_zero() {
        // Identical bars → TP constant → std_dev = 0
        let mut sig = TypicalPriceDeviation::new(3).unwrap();
        sig.update(&bar("110", "90", "100")).unwrap(); // TP=100
        sig.update(&bar("110", "90", "100")).unwrap(); // TP=100
        let v = sig.update(&bar("110", "90", "100")).unwrap(); // TP=100
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

# 3. VolumeWeightedMomentum
volume_weighted_momentum = r'''//! Volume-Weighted Momentum indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling sum of `(close - prev_close) * volume`.
///
/// Combines price direction with volume weight: bullish bars with high volume
/// contribute positively, bearish bars with high volume contribute negatively.
/// Measures directional volume pressure over the period.
pub struct VolumeWeightedMomentum {
    period: usize,
    prev_close: Option<Decimal>,
    window: VecDeque<Decimal>,
    sum: Decimal,
}

impl VolumeWeightedMomentum {
    /// Creates a new `VolumeWeightedMomentum` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self { period, prev_close: None, window: VecDeque::with_capacity(period), sum: Decimal::ZERO })
    }
}

impl Signal for VolumeWeightedMomentum {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let Some(pc) = self.prev_close {
            let wm = (bar.close - pc) * bar.volume;
            self.window.push_back(wm);
            self.sum += wm;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.sum -= old;
                }
            }
        }
        self.prev_close = Some(bar.close);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        Ok(SignalValue::Scalar(self.sum))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_close = None; self.window.clear(); self.sum = Decimal::ZERO; }
    fn name(&self) -> &str { "VolumeWeightedMomentum" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(c: &str, v: &str) -> BarInput {
        BarInput {
            open: dec!(100),
            high: dec!(110),
            low: dec!(90),
            close: c.parse().unwrap(),
            volume: v.parse().unwrap(),
        }
    }

    #[test]
    fn test_vwm_flat_zero() {
        // No price change → momentum = 0
        let mut sig = VolumeWeightedMomentum::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        sig.update(&bar("100", "1000")).unwrap();
        let v = sig.update(&bar("100", "1000")).unwrap();
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }

    #[test]
    fn test_vwm_positive_trend() {
        // Rising closes with volume → positive momentum
        let mut sig = VolumeWeightedMomentum::new(2).unwrap();
        sig.update(&bar("100", "1000")).unwrap(); // seeds prev_close=100
        sig.update(&bar("102", "1000")).unwrap(); // +2*1000=2000
        let v = sig.update(&bar("104", "1000")).unwrap(); // +2*1000=2000, window=[2000,2000], sum=4000
        assert_eq!(v, SignalValue::Scalar(dec!(4000)));
    }
}
'''

# 4. InsideBarRatio
inside_bar_ratio = r'''//! Inside Bar Ratio indicator.

use rust_decimal::Decimal;
use std::collections::VecDeque;
use crate::error::FinError;
use crate::signals::{BarInput, Signal, SignalValue};

/// Rolling fraction of bars where `high < prev_high AND low > prev_low` (inside bars).
///
/// Inside bars indicate consolidation and reduced volatility.
/// High inside bar ratios suggest market indecision or compression before a breakout.
pub struct InsideBarRatio {
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    window: VecDeque<u8>,
    count: usize,
}

impl InsideBarRatio {
    /// Creates a new `InsideBarRatio` with the given rolling period.
    pub fn new(period: usize) -> Result<Self, FinError> {
        if period == 0 {
            return Err(FinError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_high: None,
            prev_low: None,
            window: VecDeque::with_capacity(period),
            count: 0,
        })
    }
}

impl Signal for InsideBarRatio {
    fn update(&mut self, bar: &BarInput) -> Result<SignalValue, FinError> {
        if let (Some(ph), Some(pl)) = (self.prev_high, self.prev_low) {
            let inside: u8 = if bar.high < ph && bar.low > pl { 1 } else { 0 };
            self.window.push_back(inside);
            self.count += inside as usize;
            if self.window.len() > self.period {
                if let Some(old) = self.window.pop_front() {
                    self.count -= old as usize;
                }
            }
        }
        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);

        if self.window.len() < self.period {
            return Ok(SignalValue::Unavailable);
        }
        let ratio = Decimal::from(self.count as u32) / Decimal::from(self.period as u32);
        Ok(SignalValue::Scalar(ratio))
    }

    fn is_ready(&self) -> bool { self.window.len() >= self.period }
    fn period(&self) -> usize { self.period }
    fn reset(&mut self) { self.prev_high = None; self.prev_low = None; self.window.clear(); self.count = 0; }
    fn name(&self) -> &str { "InsideBarRatio" }
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
    fn test_inside_bar_ratio_all_inside() {
        let mut sig = InsideBarRatio::new(2).unwrap();
        sig.update(&bar("110", "90")).unwrap(); // seeds prev
        sig.update(&bar("108", "92")).unwrap(); // inside ✓
        let v = sig.update(&bar("106", "94")).unwrap(); // inside ✓ → 2/2 = 1
        assert_eq!(v, SignalValue::Scalar(dec!(1)));
    }

    #[test]
    fn test_inside_bar_ratio_none_inside() {
        let mut sig = InsideBarRatio::new(2).unwrap();
        sig.update(&bar("100", "95")).unwrap(); // seeds prev
        sig.update(&bar("115", "85")).unwrap(); // outside ✗
        let v = sig.update(&bar("120", "80")).unwrap(); // outside ✗ → 0/2 = 0
        assert_eq!(v, SignalValue::Scalar(dec!(0)));
    }
}
'''

files = [
    ("lower_low_count.rs", lower_low_count),
    ("typical_price_deviation.rs", typical_price_deviation),
    ("volume_weighted_momentum.rs", volume_weighted_momentum),
    ("inside_bar_ratio.rs", inside_bar_ratio),
]

for fname, content in files:
    path = os.path.join(base, fname)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(content)
    print(f"Written: {path}")

print("Done.")
